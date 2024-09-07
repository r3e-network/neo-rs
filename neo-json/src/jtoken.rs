
use std::collections::HashMap;
use std::convert::TryFrom;
use std::fmt;
use std::io::Cursor;
use std::str::FromStr;

use serde_json::{self, Value as JsonValue};

/// Represents an abstract JSON token.
#[derive(Clone, Debug)]
pub enum JToken {
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(Vec<JToken>),
    Object(HashMap<String, JToken>),
}

impl JToken {
    /// Represents a null token.
    pub const NULL: JToken = JToken::Null;

    /// Gets or sets the child token at the specified index.
    pub fn get(&self, index: usize) -> Option<&JToken> {
        match self {
            JToken::Array(arr) => arr.get(index),
            _ => None,
        }
    }

    /// Gets or sets the properties of the JSON object.
    pub fn get_property(&self, key: &str) -> Option<&JToken> {
        match self {
            JToken::Object(obj) => obj.get(key),
            _ => None,
        }
    }

    /// Converts the current JSON token to a boolean value.
    pub fn as_boolean(&self) -> bool {
        match self {
            JToken::Boolean(b) => *b,
            _ => true,
        }
    }

    /// Converts the current JSON token to an Enum.
    pub fn as_enum<T: FromStr>(&self, default_value: T, ignore_case: bool) -> T
    where
        T: Default,
    {
        match self {
            JToken::String(s) => {
                let parsed = if ignore_case {
                    T::from_str(&s.to_lowercase())
                } else {
                    T::from_str(s)
                };
                parsed.unwrap_or(default_value)
            }
            _ => default_value,
        }
    }

    /// Converts the current JSON token to a floating point number.
    pub fn as_number(&self) -> f64 {
        match self {
            JToken::Number(n) => *n,
            _ => f64::NAN,
        }
    }

    /// Converts the current JSON token to a string.
    pub fn as_string(&self) -> String {
        self.to_string()
    }

    /// Converts the current JSON token to a boolean value.
    pub fn get_boolean(&self) -> Result<bool, &'static str> {
        match self {
            JToken::Boolean(b) => Ok(*b),
            _ => Err("Invalid cast: not a JBoolean"),
        }
    }

    /// Converts the current JSON token to an Enum.
    pub fn get_enum<T: FromStr>(&self, ignore_case: bool) -> Result<T, &'static str>
    where
        T: Default,
    {
        match self {
            JToken::String(s) => {
                let parsed = if ignore_case {
                    T::from_str(&s.to_lowercase())
                } else {
                    T::from_str(s)
                };
                parsed.map_err(|_| "Invalid cast: cannot convert to enum")
            }
            _ => Err("Invalid cast: not a JString"),
        }
    }

    /// Converts the current JSON token to a 32-bit signed integer.
    pub fn get_int32(&self) -> Result<i32, &'static str> {
        match self {
            JToken::Number(n) => {
                if n.fract() == 0.0 {
                    i32::try_from(n.trunc() as i64).map_err(|_| "Overflow: cannot convert to i32")
                } else {
                    Err("Invalid cast: number is not an integer")
                }
            }
            _ => Err("Invalid cast: not a JNumber"),
        }
    }

    /// Converts the current JSON token to a floating point number.
    pub fn get_number(&self) -> Result<f64, &'static str> {
        match self {
            JToken::Number(n) => Ok(*n),
            _ => Err("Invalid cast: not a JNumber"),
        }
    }

    /// Converts the current JSON token to a string.
    pub fn get_string(&self) -> Result<String, &'static str> {
        match self {
            JToken::String(s) => Ok(s.clone()),
            _ => Err("Invalid cast: not a JString"),
        }
    }

    /// Parses a JSON token from a byte array.
    pub fn parse(value: &[u8], max_nest: usize) -> Result<JToken, String> {
        let mut de = serde_json::Deserializer::from_slice(value);
        de.disable_recursion_limit();
        let json_value: JsonValue = serde_json::Value::deserialize(&mut de)
            .map_err(|e| format!("JSON parsing error: {}", e))?;
        JToken::from_json_value(json_value, max_nest)
    }

    /// Parses a JSON token from a string.
    pub fn parse_str(value: &str, max_nest: usize) -> Result<JToken, String> {
        Self::parse(value.as_bytes(), max_nest)
    }

    fn from_json_value(value: JsonValue, max_nest: usize) -> Result<JToken, String> {
        if max_nest == 0 {
            return Err("Maximum nesting depth exceeded".to_string());
        }
        match value {
            JsonValue::Null => Ok(JToken::Null),
            JsonValue::Bool(b) => Ok(JToken::Boolean(b)),
            JsonValue::Number(n) => Ok(JToken::Number(n.as_f64().unwrap())),
            JsonValue::String(s) => Ok(JToken::String(s)),
            JsonValue::Array(arr) => {
                let mut jarray = Vec::new();
                for item in arr {
                    jarray.push(JToken::from_json_value(item, max_nest - 1)?);
                }
                Ok(JToken::Array(jarray))
            }
            JsonValue::Object(obj) => {
                let mut jobject = HashMap::new();
                for (key, value) in obj {
                    jobject.insert(key, JToken::from_json_value(value, max_nest - 1)?);
                }
                Ok(JToken::Object(jobject))
            }
        }
    }

    /// Encode the current JSON token into a byte array.
    pub fn to_byte_array(&self, indented: bool) -> Vec<u8> {
        let json_value = self.to_json_value();
        if indented {
            serde_json::to_vec_pretty(&json_value).unwrap()
        } else {
            serde_json::to_vec(&json_value).unwrap()
        }
    }

    fn to_json_value(&self) -> JsonValue {
        match self {
            JToken::Null => JsonValue::Null,
            JToken::Boolean(b) => JsonValue::Bool(*b),
            JToken::Number(n) => JsonValue::Number(serde_json::Number::from_f64(*n).unwrap()),
            JToken::String(s) => JsonValue::String(s.clone()),
            JToken::Array(arr) => JsonValue::Array(arr.iter().map(|item| item.to_json_value()).collect()),
            JToken::Object(obj) => JsonValue::Object(
                obj.iter()
                    .map(|(k, v)| (k.clone(), v.to_json_value()))
                    .collect(),
            ),
        }
    }

    /// Clone the current JSON token.
    pub fn clone(&self) -> JToken {
        self.clone()
    }

    // JsonPath implementation is omitted as it's a complex feature and may require a separate crate in Rust.
    // If needed, consider using a JsonPath library for Rust or implementing a simplified version.
}

impl fmt::Display for JToken {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let json_value = self.to_json_value();
        if f.alternate() {
            write!(f, "{}", serde_json::to_string_pretty(&json_value).unwrap())
        } else {
            write!(f, "{}", serde_json::to_string(&json_value).unwrap())
        }
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

impl From<Vec<JToken>> for JToken {
    fn from(value: Vec<JToken>) -> Self {
        JToken::Array(value)
    }
}

impl From<HashMap<String, JToken>> for JToken {
    fn from(value: HashMap<String, JToken>) -> Self {
        JToken::Object(value)
    }
}

// Additional implementations for other types can be added as needed.
