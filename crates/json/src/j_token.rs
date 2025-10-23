use crate::j_array::JArray;
use crate::j_object::JObject;
use crate::j_path_token::JPathToken;
use crate::ordered_dictionary::OrderedDictionary;
use crate::utility::Utility;
use crate::JsonError;
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use serde_json::{self, ser::PrettyFormatter};
use std::collections::HashSet;
use std::fmt;
use std::io::Write;

pub type JsonValue = Option<JToken>;

#[derive(Clone, Debug, PartialEq)]
pub enum JToken {
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(JArray),
    Object(JObject),
}

impl Serialize for JToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            JToken::Null => serializer.serialize_unit(),
            JToken::Boolean(value) => serializer.serialize_bool(*value),
            JToken::Number(value) => serializer.serialize_f64(*value),
            JToken::String(value) => serializer.serialize_str(value),
            JToken::Array(value) => {
                let mut seq = serializer.serialize_seq(Some(value.len()))?;
                for element in value.children() {
                    match element {
                        Some(token) => seq.serialize_element(token)?,
                        None => seq.serialize_element(&serde_json::Value::Null)?,
                    }
                }
                seq.end()
            }
            JToken::Object(value) => {
                let mut map = serializer.serialize_map(Some(value.len()))?;
                for (key, element) in value.iter() {
                    match element {
                        Some(token) => map.serialize_entry(key, token)?,
                        None => map.serialize_entry(key, &serde_json::Value::Null)?,
                    }
                }
                map.end()
            }
        }
    }
}

impl JToken {
    pub fn get_index(&self, index: usize) -> Result<Option<&JToken>, JsonError> {
        match self {
            JToken::Array(array) => array.get_checked(index),
            _ => Err(JsonError::NotSupported("Indexing not supported for token")),
        }
    }

    pub fn set_index(&mut self, index: usize, value: Option<JToken>) -> Result<(), JsonError> {
        match self {
            JToken::Array(array) => array.set(index, value),
            _ => Err(JsonError::NotSupported("Indexing not supported for token")),
        }
    }

    pub fn get_property(&self, key: &str) -> Result<Option<&JToken>, JsonError> {
        match self {
            JToken::Object(object) => Ok(object.get(key)),
            _ => Err(JsonError::NotSupported("Property access not supported")),
        }
    }

    pub fn set_property(
        &mut self,
        key: impl Into<String>,
        value: Option<JToken>,
    ) -> Result<(), JsonError> {
        match self {
            JToken::Object(object) => {
                object.set(key.into(), value);
                Ok(())
            }
            _ => Err(JsonError::NotSupported("Property access not supported")),
        }
    }

    pub fn as_boolean(&self) -> bool {
        match self {
            JToken::Null => false,
            JToken::Boolean(value) => *value,
            JToken::Number(value) => *value != 0.0,
            JToken::String(value) => !value.is_empty(),
            JToken::Array(array) => !array.is_empty(),
            JToken::Object(object) => !object.is_empty(),
        }
    }

    pub fn as_number(&self) -> f64 {
        match self {
            JToken::Boolean(value) => {
                if *value {
                    1.0
                } else {
                    0.0
                }
            }
            JToken::Number(value) => *value,
            JToken::String(value) => value.parse::<f64>().unwrap_or(f64::NAN),
            _ => f64::NAN,
        }
    }

    pub fn as_string(&self) -> String {
        match self {
            JToken::Null => "null".to_string(),
            JToken::Boolean(value) => value.to_string(),
            JToken::Number(value) => value.to_string(),
            JToken::String(value) => value.clone(),
            JToken::Array(array) => array.to_string(),
            JToken::Object(object) => object.to_string(),
        }
    }

    pub fn get_boolean(&self) -> Result<bool, JsonError> {
        match self {
            JToken::Boolean(value) => Ok(*value),
            _ => Err(JsonError::InvalidCast("Expected boolean token")),
        }
    }

    pub fn get_number(&self) -> Result<f64, JsonError> {
        match self {
            JToken::Number(value) => Ok(*value),
            _ => Err(JsonError::InvalidCast("Expected number token")),
        }
    }

    pub fn get_string(&self) -> Result<String, JsonError> {
        match self {
            JToken::String(value) => Ok(value.clone()),
            _ => Err(JsonError::InvalidCast("Expected string token")),
        }
    }

    pub fn get_int32(&self) -> Result<i32, JsonError> {
        let number = self.get_number()?;
        if number.fract() != 0.0 {
            return Err(JsonError::InvalidCast("Number is not integral"));
        }
        if number < i32::MIN as f64 || number > i32::MAX as f64 {
            return Err(JsonError::Overflow("Number out of range for i32"));
        }
        Ok(number as i32)
    }

    pub fn parse(value: &str, max_nest: usize) -> Result<JToken, JsonError> {
        Self::parse_bytes(value.as_bytes(), max_nest)
    }

    pub fn parse_bytes(bytes: &[u8], max_nest: usize) -> Result<JToken, JsonError> {
        let mut deserializer = serde_json::Deserializer::from_slice(bytes);
        let seed = TokenSeed {
            remaining_depth: max_nest,
            max_depth: max_nest,
        };
        let token = seed.deserialize(&mut deserializer)?;
        deserializer.end()?;
        Ok(token.unwrap_or(JToken::Null))
    }

    pub fn to_byte_array(&self, indented: bool) -> Result<Vec<u8>, JsonError> {
        let mut buffer = Vec::new();
        if indented {
            let formatter = PrettyFormatter::with_indent(b"  ");
            let mut serializer = serde_json::Serializer::with_formatter(&mut buffer, formatter);
            self.serialize(&mut serializer)?;
        } else {
            let mut serializer = serde_json::Serializer::new(&mut buffer);
            self.serialize(&mut serializer)?;
        }
        Ok(buffer)
    }

    pub fn to_string_formatted(&self, indented: bool) -> Result<String, JsonError> {
        let bytes = self.to_byte_array(indented)?;
        Utility::strict_utf8_decode(&bytes).map_err(JsonError::from)
    }

    pub fn write(&self, writer: &mut dyn Write, indented: bool) -> Result<(), JsonError> {
        if indented {
            let formatter = PrettyFormatter::with_indent(b"  ");
            let mut serializer = serde_json::Serializer::with_formatter(writer, formatter);
            self.serialize(&mut serializer)?;
        } else {
            let mut serializer = serde_json::Serializer::new(writer);
            self.serialize(&mut serializer)?;
        }
        Ok(())
    }

    pub fn json_path(&self, expr: &str) -> Result<JArray, JsonError> {
        let mut objects = vec![Some(self.clone())];
        if expr.is_empty() {
            return Ok(JArray::from_vec(objects));
        }

        let tokens = JPathToken::parse(expr);
        let mut queue: std::collections::VecDeque<_> = tokens.into();
        let first = queue
            .pop_front()
            .ok_or_else(|| JsonError::format("JsonPath expression is empty"))?;
        if !first.is_root() {
            return Err(JsonError::format("JsonPath must start with '$'"));
        }
        JPathToken::process_json_path(&mut objects, queue);
        Ok(JArray::from_vec(objects))
    }
}

impl From<bool> for JToken {
    fn from(value: bool) -> Self {
        JToken::Boolean(value)
    }
}

impl From<f64> for JToken {
    fn from(value: f64) -> Self {
        JToken::Number(value)
    }
}

impl From<String> for JToken {
    fn from(value: String) -> Self {
        JToken::String(value)
    }
}

impl From<&str> for JToken {
    fn from(value: &str) -> Self {
        JToken::String(value.to_string())
    }
}

impl From<JArray> for JToken {
    fn from(value: JArray) -> Self {
        JToken::Array(value)
    }
}
impl From<JObject> for JToken {
    fn from(value: JObject) -> Self {
        JToken::Object(value)
    }
}

impl From<Vec<Option<JToken>>> for JToken {
    fn from(value: Vec<Option<JToken>>) -> Self {
        JToken::Array(JArray::from_vec(value))
    }
}

impl fmt::Display for JToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match serde_json::to_string(self) {
            Ok(text) => f.write_str(&text),
            Err(_) => Err(fmt::Error),
        }
    }
}

impl<'de> serde::Deserialize<'de> for JToken {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let seed = TokenSeed {
            remaining_depth: usize::MAX,
            max_depth: usize::MAX,
        };
        seed.deserialize(deserializer)
            .map(|value| value.unwrap_or(JToken::Null))
    }
}

struct TokenSeed {
    remaining_depth: usize,
    max_depth: usize,
}

impl<'de> DeserializeSeed<'de> for TokenSeed {
    type Value = JsonValue;

    fn deserialize<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        if self.remaining_depth == 0 {
            return Err(de::Error::custom("Exceeded max depth"));
        }
        let visitor = TokenVisitor {
            remaining_depth: self.remaining_depth,
            max_depth: self.max_depth,
        };
        deserializer.deserialize_any(visitor)
    }
}

struct TokenVisitor {
    remaining_depth: usize,
    max_depth: usize,
}

impl<'de> Visitor<'de> for TokenVisitor {
    type Value = JsonValue;

    fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
        formatter.write_str("a JSON value")
    }

    fn visit_bool<E>(self, value: bool) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Some(JToken::Boolean(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Some(JToken::Number(value as f64)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Some(JToken::Number(value as f64)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Some(JToken::Number(value)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Some(JToken::String(value.to_string())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(Some(JToken::String(value)))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(None)
    }

    fn visit_none<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        Ok(None)
    }

    fn visit_some<D>(self, deserializer: D) -> Result<Self::Value, D::Error>
    where
        D: Deserializer<'de>,
    {
        let seed = TokenSeed {
            remaining_depth: self.remaining_depth,
            max_depth: self.max_depth,
        };
        seed.deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        if self.remaining_depth == 0 {
            return Err(de::Error::custom("Exceeded max depth"));
        }

        let mut items = Vec::new();
        while let Some(item) = seq.next_element_seed(TokenSeed {
            remaining_depth: self.remaining_depth - 1,
            max_depth: self.max_depth,
        })? {
            items.push(item);
        }

        Ok(Some(JToken::Array(JArray::from_vec(items))))
    }

    fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
    where
        A: MapAccess<'de>,
    {
        if self.remaining_depth == 0 {
            return Err(de::Error::custom("Exceeded max depth"));
        }

        let mut properties = OrderedDictionary::new();
        let mut seen = HashSet::new();

        while let Some(key) = map.next_key::<String>()? {
            if !seen.insert(key.clone()) {
                return Err(de::Error::custom(format!("Duplicate property name: {key}")));
            }
            let value = map.next_value_seed(TokenSeed {
                remaining_depth: self.remaining_depth - 1,
                max_depth: self.max_depth,
            })?;
            if !properties.add(key.clone(), value) {
                return Err(de::Error::custom(format!("Duplicate property name: {key}")));
            }
        }

        Ok(Some(JToken::Object(JObject::from_properties(properties))))
    }
}
