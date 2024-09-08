use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use crate::jpath_token::{JPath};
use crate::json_error::JsonError;

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
            JToken::Boolean(b) => if *b { 1.0 } else { 0.0 },
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
        if n.fract() != 0.0 {
            return Err(JsonError::InvalidCast);
        }
        n.try_into().map_err(|_| JsonError::InvalidCast)
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
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "{}", item)?;
                }
                write!(f, "]")
            },
            JToken::Object(o) => {
                write!(f, "{{")?;
                for (i, (key, value)) in o.iter().enumerate() {
                    if i > 0 { write!(f, ", ")?; }
                    write!(f, "\"{}\": {}", key, value)?;
                }
                write!(f, "}}")
            },
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
            Value::Object(o) => JToken::Object(o.into_iter().map(|(k, v)| (k, JToken::from(v))).collect()),
        }
    }
}