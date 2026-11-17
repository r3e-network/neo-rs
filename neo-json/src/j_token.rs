use crate::JsonError;
use crate::j_array::JArray;
use crate::j_object::JObject;
use crate::j_path_token::JPathToken;
use crate::ordered_dictionary::OrderedDictionary;

/// Largest safe integer in JSON (2^53 - 1).
pub const MAX_SAFE_INTEGER: i64 = (1i64 << 53) - 1;

/// Smallest safe integer in JSON (-(2^53 - 1)).
pub const MIN_SAFE_INTEGER: i64 = -MAX_SAFE_INTEGER;
use serde::de::{self, DeserializeSeed, Deserializer, MapAccess, SeqAccess, Visitor};
use serde::ser::{Serialize, SerializeMap, SerializeSeq, Serializer};
use serde_json::{self, ser::PrettyFormatter};
use std::fmt;
use std::io::Write;
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

/// An optional JSON token, representing a value that may be absent.
pub type JsonValue = Option<JToken>;

/// Resource limits applied while parsing JSON into a [`JToken`].
///
/// Nesting depth is passed separately (`max_nest`) because it is part of the
/// historical `JToken::parse(value, max_nest)` contract. Everything else that
/// lets a small input amplify into a large allocation is bounded here: the raw
/// input size, individual string lengths, the width of a single container and
/// the total number of nodes in the document.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ParseLimits {
    /// Maximum accepted size of the input buffer in bytes.
    pub max_input_bytes: usize,
    /// Maximum length in bytes of a single JSON string value or object key.
    pub max_string_length: usize,
    /// Maximum number of members in one object, or elements in one array.
    pub max_members: usize,
    /// Maximum number of JSON nodes (scalars plus containers) in the document.
    pub max_total_nodes: usize,
}

impl ParseLimits {
    /// Creates an explicit set of limits.
    #[must_use]
    pub const fn new(
        max_input_bytes: usize,
        max_string_length: usize,
        max_members: usize,
        max_total_nodes: usize,
    ) -> Self {
        Self {
            max_input_bytes,
            max_string_length,
            max_members,
            max_total_nodes,
        }
    }

    /// Limits that disable every bound.
    ///
    /// Only appropriate when the input is already trusted *and* already
    /// materialised in memory (for example the blanket `Deserialize` impl, which
    /// receives values that `serde_json` has parsed and validated itself).
    #[must_use]
    pub const fn unbounded() -> Self {
        Self::new(usize::MAX, usize::MAX, usize::MAX, usize::MAX)
    }
}

impl Default for ParseLimits {
    fn default() -> Self {
        // Conservative defaults sized for RPC payloads: larger than any sane
        // request body, small enough that amplification attacks cannot allocate
        // gigabytes before failing.
        Self::new(16 * 1024 * 1024, 4 * 1024 * 1024, 1 << 20, 1 << 20)
    }
}

/// Core JSON token enum (matches C# `Neo.Json.JToken`).
#[derive(Clone, Debug, PartialEq)]
pub enum JToken {
    /// A JSON `null` value.
    Null,
    /// A JSON boolean value.
    Boolean(bool),
    /// A JSON numeric value stored as `f64`.
    Number(f64),
    /// A JSON string value.
    String(String),
    /// A JSON array (matches C# `JArray`).
    Array(JArray),
    /// A JSON object (matches C# `JObject`).
    Object(JObject),
}

impl Serialize for JToken {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        match self {
            Self::Null => serializer.serialize_unit(),
            Self::Boolean(value) => serializer.serialize_bool(*value),
            Self::Number(value) => {
                if value.is_finite()
                    && value.fract() == 0.0
                    && *value >= i64::MIN as f64
                    && *value <= i64::MAX as f64
                {
                    serializer.serialize_i64(*value as i64)
                } else {
                    serializer.serialize_f64(*value)
                }
            }
            Self::String(value) => serializer.serialize_str(value),
            Self::Array(value) => {
                let mut seq = serializer.serialize_seq(Some(value.len()))?;
                for element in value.children() {
                    match element {
                        Some(token) => seq.serialize_element(token)?,
                        None => seq.serialize_element(&serde_json::Value::Null)?,
                    }
                }
                seq.end()
            }
            Self::Object(value) => {
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
    /// Helper constructor for JSON objects from ordered dictionaries.
    #[must_use]
    pub fn from_object(properties: OrderedDictionary<String, Option<Self>>) -> Self {
        Self::Object(JObject::from(properties))
    }

    /// Helper constructor for JSON arrays from optional token vectors.
    #[must_use]
    pub fn from_array(items: Vec<Option<Self>>) -> Self {
        Self::Array(JArray::from(items))
    }

    /// Helper constructor for JSON arrays from token vectors.
    #[must_use]
    pub fn from_array_tokens(items: Vec<Self>) -> Self {
        Self::Array(JArray::from(items))
    }

    /// Returns the element at the given index if this token is an array.
    pub fn get_index(&self, index: usize) -> Result<Option<&Self>, JsonError> {
        match self {
            Self::Array(array) => array.get_checked(index),
            _ => Err(JsonError::NotSupported("Indexing not supported for token")),
        }
    }

    /// Sets the element at the given index if this token is an array.
    pub fn set_index(&mut self, index: usize, value: Option<Self>) -> Result<(), JsonError> {
        match self {
            Self::Array(array) => array.set(index, value),
            _ => Err(JsonError::NotSupported("Indexing not supported for token")),
        }
    }

    /// Returns the value of the given property key if this token is an object.
    pub fn get_property(&self, key: &str) -> Result<Option<&Self>, JsonError> {
        match self {
            Self::Object(object) => Ok(object.get(key)),
            _ => Err(JsonError::NotSupported("Property access not supported")),
        }
    }

    /// Sets the value of the given property key if this token is an object.
    pub fn set_property(
        &mut self,
        key: impl Into<String>,
        value: Option<Self>,
    ) -> Result<(), JsonError> {
        match self {
            Self::Object(object) => {
                object.set(key.into(), value);
                Ok(())
            }
            _ => Err(JsonError::NotSupported("Property access not supported")),
        }
    }

    /// Attempts to view this token as a JSON array.
    #[must_use]
    pub const fn as_array(&self) -> Option<&JArray> {
        match self {
            Self::Array(array) => Some(array),
            _ => None,
        }
    }

    /// Attempts to view this token as a JSON object.
    #[must_use]
    pub const fn as_object(&self) -> Option<&JObject> {
        match self {
            Self::Object(object) => Some(object),
            _ => None,
        }
    }

    /// Returns the token's truthiness as a boolean (matches C# `AsBoolean`).
    #[must_use]
    pub fn as_boolean(&self) -> bool {
        match self {
            Self::Null => false,
            Self::Boolean(value) => *value,
            Self::Number(value) => *value != 0.0,
            Self::String(value) => !value.is_empty(),
            Self::Array(array) => !array.is_empty(),
            Self::Object(object) => !object.is_empty(),
        }
    }

    /// Attempts to convert the token to an `f64` numeric value.
    #[must_use]
    pub fn as_number(&self) -> Option<f64> {
        match self {
            Self::Boolean(value) => {
                if *value {
                    Some(1.0)
                } else {
                    Some(0.0)
                }
            }
            Self::Number(value) => Some(*value),
            Self::String(value) => value.parse::<f64>().ok(),
            _ => None,
        }
    }

    /// Returns the underlying string value if the token represents a JSON string.
    #[must_use]
    pub fn as_string(&self) -> Option<String> {
        match self {
            Self::String(value) => Some(value.clone()),
            _ => None,
        }
    }

    /// Converts the token to a string representation (used for diagnostics/logging).
    #[must_use]
    pub fn to_string_value(&self) -> String {
        match self {
            Self::Null => "null".to_string(),
            Self::Boolean(value) => value.to_string(),
            Self::Number(value) => value.to_string(),
            Self::String(value) => value.clone(),
            Self::Array(array) => array.to_string(),
            Self::Object(object) => object.to_string(),
        }
    }

    /// Returns the boolean value or an error if the token is not a boolean.
    pub const fn try_as_boolean(&self) -> Result<bool, JsonError> {
        match self {
            Self::Boolean(value) => Ok(*value),
            _ => Err(JsonError::InvalidCast("Expected boolean token")),
        }
    }

    /// Returns the numeric value or an error if the token is not a number.
    pub const fn try_as_number(&self) -> Result<f64, JsonError> {
        match self {
            Self::Number(value) => Ok(*value),
            _ => Err(JsonError::InvalidCast("Expected number token")),
        }
    }

    /// Returns the string value or an error if the token is not a string.
    pub fn try_as_string(&self) -> Result<String, JsonError> {
        match self {
            Self::String(value) => Ok(value.clone()),
            _ => Err(JsonError::InvalidCast("Expected string token")),
        }
    }

    /// Returns the numeric value as an `i32`, or an error if not integral or out of range.
    pub fn try_as_int32(&self) -> Result<i32, JsonError> {
        let number = self.try_as_number()?;
        if number.fract() != 0.0 {
            return Err(JsonError::InvalidCast("Number is not integral"));
        }
        if number < f64::from(i32::MIN) || number > f64::from(i32::MAX) {
            return Err(JsonError::Overflow("Number out of range for i32"));
        }
        Ok(number as i32)
    }

    /// Parses a JSON string into a `JToken` with a maximum nesting depth.
    ///
    /// Applies [`ParseLimits::default`] to bound input bytes, string lengths,
    /// container widths and total node count.
    pub fn parse(value: &str, max_nest: usize) -> Result<Self, JsonError> {
        Self::parse_with_limits(value, max_nest, ParseLimits::default())
    }

    /// Parses a JSON string into a `JToken` with explicit limits.
    pub fn parse_with_limits(
        value: &str,
        max_nest: usize,
        limits: ParseLimits,
    ) -> Result<Self, JsonError> {
        Self::parse_bytes_with_limits(value.as_bytes(), max_nest, limits)
    }

    /// Parses a JSON byte slice into a `JToken` with a maximum nesting depth.
    ///
    /// Applies [`ParseLimits::default`] to bound input bytes, string lengths,
    /// container widths and total node count.
    pub fn parse_bytes(bytes: &[u8], max_nest: usize) -> Result<Self, JsonError> {
        Self::parse_bytes_with_limits(bytes, max_nest, ParseLimits::default())
    }

    /// Parses a JSON byte slice into a `JToken` with explicit limits.
    ///
    /// The input length is rejected up front, before any deserializer work, so an
    /// oversized body never reaches the allocation-heavy paths.
    pub fn parse_bytes_with_limits(
        bytes: &[u8],
        max_nest: usize,
        limits: ParseLimits,
    ) -> Result<Self, JsonError> {
        if bytes.len() > limits.max_input_bytes {
            return Err(JsonError::LimitExceeded(format!(
                "input of {} bytes exceeds maximum of {} bytes",
                bytes.len(),
                limits.max_input_bytes
            )));
        }

        let mut deserializer = serde_json::Deserializer::from_slice(bytes);
        let seed = TokenSeed {
            remaining_depth: max_nest,
            max_depth: max_nest,
            limits,
            nodes: Arc::new(AtomicUsize::new(0)),
        };
        let token = seed.deserialize(&mut deserializer)?;
        deserializer.end()?;
        Ok(token.unwrap_or(Self::Null))
    }

    /// Serializes this token to a UTF-8 byte vector, optionally with indentation.
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

    /// Serializes this token to a JSON string, optionally with indentation.
    pub fn to_string_formatted(&self, indented: bool) -> Result<String, JsonError> {
        let bytes = self.to_byte_array(indented)?;
        String::from_utf8(bytes).map_err(JsonError::from)
    }

    /// Writes this token as JSON to the given writer, optionally with indentation.
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

    /// Evaluates a JSONPath expression against this token and returns matching elements.
    pub fn json_path(&self, expr: &str) -> Result<JArray, JsonError> {
        if expr.is_empty() {
            return Ok(JArray::from_vec(vec![Some(self.clone())]));
        }

        let tokens = JPathToken::parse(expr)?;
        let results = JPathToken::evaluate(&tokens, self)?;
        let collected = results
            .into_iter()
            .map(|token| Some(token.clone()))
            .collect();
        Ok(JArray::from_vec(collected))
    }
}

impl From<bool> for JToken {
    fn from(value: bool) -> Self {
        Self::Boolean(value)
    }
}

impl From<f64> for JToken {
    fn from(value: f64) -> Self {
        Self::Number(value)
    }
}

impl From<String> for JToken {
    fn from(value: String) -> Self {
        Self::String(value)
    }
}

impl From<&str> for JToken {
    fn from(value: &str) -> Self {
        Self::String(value.to_string())
    }
}

impl From<JArray> for JToken {
    fn from(value: JArray) -> Self {
        Self::Array(value)
    }
}
impl From<JObject> for JToken {
    fn from(value: JObject) -> Self {
        Self::Object(value)
    }
}

impl From<Vec<Option<Self>>> for JToken {
    fn from(value: Vec<Option<Self>>) -> Self {
        Self::Array(JArray::from_vec(value))
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
        // Parsing a `serde_json::Value` back into a `JToken` operates on data the
        // caller already materialised and validated, so no extra budget is
        // applied here; use `parse_with_limits` for untrusted byte input.
        let seed = TokenSeed {
            remaining_depth: usize::MAX,
            max_depth: usize::MAX,
            limits: ParseLimits::unbounded(),
            nodes: Arc::new(AtomicUsize::new(0)),
        };
        seed.deserialize(deserializer)
            .map(|value| value.unwrap_or(Self::Null))
    }
}

#[derive(Clone)]
struct TokenSeed {
    remaining_depth: usize,
    max_depth: usize,
    limits: ParseLimits,
    nodes: Arc<AtomicUsize>,
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
            limits: self.limits,
            nodes: self.nodes,
        };
        deserializer.deserialize_any(visitor)
    }
}

struct TokenVisitor {
    remaining_depth: usize,
    max_depth: usize,
    limits: ParseLimits,
    nodes: Arc<AtomicUsize>,
}

impl TokenVisitor {
    /// Charges one JSON node against the document-wide node budget.
    fn charge_node<E>(&self) -> Result<(), E>
    where
        E: de::Error,
    {
        let total = self.nodes.fetch_add(1, Ordering::Relaxed).saturating_add(1);
        if total > self.limits.max_total_nodes {
            return Err(de::Error::custom(format!(
                "Exceeded maximum node count ({})",
                self.limits.max_total_nodes
            )));
        }
        Ok(())
    }

    /// Rejects strings (and object keys) longer than the string budget.
    fn charge_string<E>(&self, value: &str) -> Result<(), E>
    where
        E: de::Error,
    {
        if value.len() > self.limits.max_string_length {
            return Err(de::Error::custom(format!(
                "String length {} exceeds maximum of {}",
                value.len(),
                self.limits.max_string_length
            )));
        }
        Ok(())
    }

    /// Rejects containers wider than the member budget.
    fn charge_member<E>(&self, len: usize) -> Result<(), E>
    where
        E: de::Error,
    {
        if len > self.limits.max_members {
            return Err(de::Error::custom(format!(
                "Container member count {len} exceeds maximum of {}",
                self.limits.max_members
            )));
        }
        Ok(())
    }

    fn child_seed(&self) -> TokenSeed {
        TokenSeed {
            remaining_depth: self.remaining_depth.saturating_sub(1),
            max_depth: self.max_depth,
            limits: self.limits,
            nodes: Arc::clone(&self.nodes),
        }
    }
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
        self.charge_node()?;
        Ok(Some(JToken::Boolean(value)))
    }

    fn visit_i64<E>(self, value: i64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_node()?;
        Ok(Some(JToken::Number(value as f64)))
    }

    fn visit_u64<E>(self, value: u64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_node()?;
        Ok(Some(JToken::Number(value as f64)))
    }

    fn visit_f64<E>(self, value: f64) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_node()?;
        Ok(Some(JToken::Number(value)))
    }

    fn visit_str<E>(self, value: &str) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_string(value)?;
        self.charge_node()?;
        Ok(Some(JToken::String(value.to_string())))
    }

    fn visit_string<E>(self, value: String) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_string(&value)?;
        self.charge_node()?;
        Ok(Some(JToken::String(value)))
    }

    fn visit_unit<E>(self) -> Result<Self::Value, E>
    where
        E: de::Error,
    {
        self.charge_node()?;
        Ok(Some(JToken::Null))
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
        let seed = self.child_seed();
        seed.deserialize(deserializer)
    }

    fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
    where
        A: SeqAccess<'de>,
    {
        if self.remaining_depth == 0 {
            return Err(de::Error::custom("Exceeded max depth"));
        }
        self.charge_node()?;

        let seed = self.child_seed();
        let mut items = Vec::new();
        while let Some(item) = seq.next_element_seed(seed.clone())? {
            self.charge_member(items.len().saturating_add(1))?;
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
        self.charge_node()?;

        let seed = self.child_seed();
        let mut properties = OrderedDictionary::new();
        while let Some(key) = map.next_key::<String>()? {
            self.charge_string(&key)?;
            self.charge_member(properties.len().saturating_add(1))?;
            let value = map.next_value_seed(seed.clone())?;
            if let Err((key, _)) = properties.try_insert(key, value) {
                return Err(de::Error::custom(format!("Duplicate property name: {key}")));
            }
        }

        Ok(Some(JToken::Object(JObject::from_properties(properties))))
    }
}

#[cfg(test)]
mod parse_limits_tests {
    use super::{JToken, ParseLimits};
    use crate::JsonError;

    fn limits(
        max_input_bytes: usize,
        max_string_length: usize,
        max_members: usize,
        max_total_nodes: usize,
    ) -> ParseLimits {
        ParseLimits::new(
            max_input_bytes,
            max_string_length,
            max_members,
            max_total_nodes,
        )
    }

    #[test]
    fn default_limits_accept_small_documents() {
        assert_eq!(JToken::parse("0", 1).expect("scalar"), JToken::Number(0.0));
        assert!(JToken::parse("[0]", 2).is_ok());
        assert!(JToken::parse_bytes(br#"{"a":[1,2,3],"b":null}"#, 3).is_ok());
    }

    #[test]
    fn input_byte_budget_rejects_before_deserializing() {
        let err = JToken::parse_bytes_with_limits(b"[1,2,3]", 4, limits(4, 64, 64, 64))
            .expect_err("input over budget");
        match err {
            JsonError::LimitExceeded(message) => {
                assert!(message.contains("exceeds maximum of 4 bytes"), "{message}");
            }
            other => panic!("unexpected error: {other:?}"),
        }
    }

    #[test]
    fn string_length_budget_rejects_long_values_and_keys() {
        let err = JToken::parse_with_limits("\"abcdef\"", 4, limits(1024, 3, 64, 64))
            .expect_err("string over budget");
        assert!(err.to_string().contains("String length 6 exceeds maximum of 3"));

        let err = JToken::parse_with_limits(
            r#"{"abcdef":1}"#,
            4,
            limits(1024, 3, 64, 64),
        )
        .expect_err("key over budget");
        assert!(err.to_string().contains("String length 6 exceeds maximum of 3"));
    }

    #[test]
    fn member_budget_rejects_wide_containers() {
        let err = JToken::parse_with_limits("[1,2,3,4]", 4, limits(1024, 64, 3, 64))
            .expect_err("array too wide");
        assert!(err.to_string().contains("member count 4 exceeds maximum of 3"));

        let err = JToken::parse_with_limits(
            r#"{"a":1,"b":2,"c":3}"#,
            4,
            limits(1024, 64, 2, 64),
        )
        .expect_err("object too wide");
        assert!(err.to_string().contains("member count 3 exceeds maximum of 2"));
    }

    #[test]
    fn node_budget_rejects_oversized_documents() {
        let err = JToken::parse_with_limits("[1,2,3]", 4, limits(1024, 64, 64, 3))
            .expect_err("too many nodes");
        assert!(err.to_string().contains("Exceeded maximum node count (3)"));
    }

    #[test]
    fn unbounded_limits_accept_any_document() {
        let token = JToken::parse_with_limits("[[1,2],[3]]", 8, ParseLimits::unbounded())
            .expect("unbounded parse");
        assert!(!token.to_string_value().is_empty());
    }
}
