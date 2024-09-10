use crate::jpath_token::JPath;
use crate::json_error::JsonError;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::convert::{TryFrom, TryInto};
/// Represents the largest safe integer in JSON.
pub const MAX_SAFE_INTEGER: i64 = (1 << 53) - 1;

/// Represents the smallest safe integer in JSON.
pub const MIN_SAFE_INTEGER: i64 = -((1 << 53) - 1);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum JToken {
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Array(Vec<JToken>),
    Object(IndexMap<String, JToken>),
}

impl JToken {
    pub fn parse(s: &str) -> Result<Self, JsonError> {
        let value: Value = serde_json::from_str(s)?;
        Ok(Self::from(value))
    }

    pub fn parse_bytes(bytes: &[u8]) -> Result<Self, JsonError> {
        let value: Value = serde_json::from_slice(bytes)?;
        Ok(Self::from(value))
    }

    pub fn as_boolean(&self) -> bool {
        match self {
            JToken::Boolean(b) => *b,
            JToken::Number(n) => *n != 0.0,
            JToken::String(s) => !s.is_empty(),
            JToken::Array(a) => !a.is_empty(),
            JToken::Object(o) => !o.is_empty(),
            JToken::Null => false,
        }
    }

    pub fn as_number(&self) -> f64 {
        match self {
            JToken::Number(n) => *n,
            JToken::Boolean(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
            JToken::String(s) => s.parse().unwrap_or(f64::NAN),
            _ => f64::NAN,
        }
    }

    pub fn as_string(&self) -> String {
        match self {
            JToken::String(s) => s.clone(),
            _ => self.to_string(),
        }
    }

    pub fn get_boolean(&self) -> Result<bool, JsonError> {
        match self {
            JToken::Boolean(b) => Ok(*b),
            _ => Err(JsonError::InvalidCast),
        }
    }

    pub fn json_path(&self, expr: &str) -> Result<Vec<JToken>, JsonError> {
        let path = expr.parse::<JPath>()?;
        path.apply(self)
    }

    pub fn get_number(&self) -> Result<f64, JsonError> {
        match self {
            JToken::Number(n) => Ok(*n),
            _ => Err(JsonError::InvalidCast),
        }
    }

    pub fn get_string(&self) -> Result<String, JsonError> {
        match self {
            JToken::String(s) => Ok(s.clone()),
            _ => Err(JsonError::InvalidCast),
        }
    }

    pub fn get_int32(&self) -> Result<i32, JsonError> {
        let n = self.get_number()?;
        if n.is_finite() && n >= i32::MIN as f64 && n <= i32::MAX as f64 {
            Ok(n as i32)
        } else {
            Err(JsonError::InvalidCast)
        }
    }

    pub fn get(&self, key: &str) -> Result<&JToken, JsonError> {
        match self {
            JToken::Object(map) => map.get(key).ok_or(JsonError::KeyNotFound),
            _ => Err(JsonError::InvalidCast),
        }
    }

    pub fn get_mut(&mut self, key: &str) -> Result<&mut JToken, JsonError> {
        match self {
            JToken::Object(map) => map.get_mut(key).ok_or(JsonError::KeyNotFound),
            _ => Err(JsonError::InvalidCast),
        }
    }

    pub fn get_index(&self, index: usize) -> Result<&JToken, JsonError> {
        match self {
            JToken::Array(vec) => vec.get(index).ok_or(JsonError::IndexOutOfBounds),
            _ => Err(JsonError::InvalidCast),
        }
    }

    pub fn get_index_mut(&mut self, index: usize) -> Result<&mut JToken, JsonError> {
        match self {
            JToken::Array(vec) => vec.get_mut(index).ok_or(JsonError::IndexOutOfBounds),
            _ => Err(JsonError::InvalidCast),
        }
    }

    pub fn is_null(&self) -> bool {
        matches!(self, JToken::Null)
    }

    pub fn is_boolean(&self) -> bool {
        matches!(self, JToken::Boolean(_))
    }

    pub fn is_number(&self) -> bool {
        matches!(self, JToken::Number(_))
    }

    pub fn is_string(&self) -> bool {
        matches!(self, JToken::String(_))
    }

    pub fn is_array(&self) -> bool {
        matches!(self, JToken::Array(_))
    }

    pub fn is_object(&self) -> bool {
        matches!(self, JToken::Object(_))
    }

    pub fn to_byte_array(&self, indented: bool) -> Vec<u8> {
        if indented {
            serde_json::to_vec_pretty(self).unwrap_or_default()
        } else {
            serde_json::to_vec(self).unwrap_or_default()
        }
    }

    pub fn to_string_pretty(&self) -> String {
        serde_json::to_string_pretty(self).unwrap_or_default()
    }

    pub fn clone(&self) -> JToken {
        self.clone()
    }

    pub fn new_object() -> Self {
        JToken::Object(IndexMap::new())
    }

    pub fn new_array() -> Self {
        JToken::Array(Vec::new())
    }
    pub fn insert(&mut self, key: String, value: JToken) -> Result<Self, JsonError> {
        match self {
            JToken::Object(map) => {
                map.insert(key, value);
                Ok(Self::clone(self))
            }
            _ => Err(JsonError::InvalidCast),
        }
    }

    pub fn push(&mut self, value: JToken) -> Result<Self, JsonError> {
        match self {
            JToken::Array(vec) => {
                vec.push(value);
                Ok(Self::clone(self))
            }
            _ => Err(JsonError::InvalidCast),
        }
    }

    pub fn remove(&mut self, key: &str) -> Result<JToken, JsonError> {
        match self {
            JToken::Object(map) => map.remove(key).ok_or(JsonError::KeyNotFound),
            _ => Err(JsonError::InvalidCast),
        }
    }

    pub fn remove_index(&mut self, index: usize) -> Result<JToken, JsonError> {
        match self {
            JToken::Array(vec) => {
                if index < vec.len() {
                    Ok(vec.remove(index))
                } else {
                    Err(JsonError::IndexOutOfBounds)
                }
            }
            _ => Err(JsonError::InvalidCast),
        }
    }
}

impl std::fmt::Display for JToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            JToken::Null => write!(f, "null"),
            JToken::Boolean(b) => write!(f, "{}", b),
            JToken::Number(n) => write!(f, "{}", n),
            JToken::String(s) => write!(f, "\"{}\"", s),
            JToken::Array(a) => {
                write!(f, "[")?;
                for (i, item) in a.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            }
            JToken::Object(o) => {
                write!(f, "{{")?;
                for (i, (key, value)) in o.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "\"{}\": {}", key, value)?;
                }
                write!(f, "}}")
            }
        }
    }
}

impl From<Value> for JToken {
    fn from(value: Value) -> Self {
        match value {
            Value::Null => JToken::Null,
            Value::Bool(b) => JToken::Boolean(b),
            Value::Number(n) => JToken::Number(n.as_f64().unwrap_or(f64::NAN)),
            Value::String(s) => JToken::String(s),
            Value::Array(a) => JToken::Array(a.into_iter().map(JToken::from).collect()),
            Value::Object(o) => {
                JToken::Object(o.into_iter().map(|(k, v)| (k, JToken::from(v))).collect())
            }
        }
    }
}

// Implement TryFrom for various types
impl TryFrom<JToken> for bool {
    type Error = JsonError;

    fn try_from(value: JToken) -> Result<Self, Self::Error> {
        if let JToken::Boolean(b) = value {
            Ok(b)
        } else {
            Err(JsonError::InvalidCast)
        }
    }
}

impl TryFrom<JToken> for f64 {
    type Error = JsonError;

    fn try_from(value: JToken) -> Result<Self, Self::Error> {
        if let JToken::Number(n) = value {
            Ok(n)
        } else {
            Err(JsonError::InvalidCast)
        }
    }
}

impl TryFrom<JToken> for String {
    type Error = JsonError;

    fn try_from(value: JToken) -> Result<Self, Self::Error> {
        if let JToken::String(s) = value {
            Ok(s)
        } else {
            Err(JsonError::InvalidCast)
        }
    }
}

impl TryFrom<JToken> for Vec<JToken> {
    type Error = JsonError;

    fn try_from(value: JToken) -> Result<Self, Self::Error> {
        if let JToken::Array(a) = value {
            Ok(a)
        } else {
            Err(JsonError::InvalidCast)
        }
    }
}

impl TryFrom<JToken> for IndexMap<String, JToken> {
    type Error = JsonError;

    fn try_from(value: JToken) -> Result<Self, Self::Error> {
        if let JToken::Object(o) = value {
            Ok(o)
        } else {
            Err(JsonError::InvalidCast)
        }
    }
}

// Implement From for various types
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

impl From<i32> for JToken {
    fn from(value: i32) -> Self {
        JToken::Number(value as f64)
    }
}

impl From<i64> for JToken {
    fn from(value: i64) -> Self {
        JToken::Number(value as f64)
    }
}

impl From<u32> for JToken {
    fn from(value: u32) -> Self {
        JToken::Number(value as f64)
    }
}

impl From<u64> for JToken {
    fn from(value: u64) -> Self {
        JToken::Number(value as f64)
    }
}

impl From<i16> for JToken {
    fn from(value: i16) -> Self {
        JToken::Number(value as f64)
    }
}

impl From<u16> for JToken {
    fn from(value: u16) -> Self {
        JToken::Number(value as f64)
    }
}

impl From<i8> for JToken {
    fn from(value: i8) -> Self {
        JToken::Number(value as f64)
    }
}

impl From<u8> for JToken {
    fn from(value: u8) -> Self {
        JToken::Number(value as f64)
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

impl From<IndexMap<String, JToken>> for JToken {
    fn from(value: IndexMap<String, JToken>) -> Self {
        JToken::Object(value)
    }
}
