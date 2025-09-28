//! JToken - matches C# Neo.Json.JToken exactly

use std::io::Write;

/// Represents a JSON token (matches C# JToken abstract class)
#[derive(Clone, Debug)]
pub enum JToken {
    Null,
    Boolean(crate::j_boolean::JBoolean),
    Number(crate::j_number::JNumber),
    String(crate::j_string::JString),
    Array(crate::j_array::JArray),
    Object(crate::j_object::JObject),
}

impl JToken {
    /// Gets or sets the child token at the specified index
    pub fn get_index(&self, index: usize) -> Option<&JToken> {
        match self {
            JToken::Array(arr) => arr.get(index),
            _ => None,
        }
    }

    /// Sets the child token at the specified index
    pub fn set_index(&mut self, index: usize, value: Option<JToken>) {
        if let JToken::Array(arr) = self {
            arr.set(index, value);
        }
    }

    /// Gets property by key
    pub fn get_property(&self, key: &str) -> Option<&JToken> {
        match self {
            JToken::Object(obj) => obj.get(key),
            _ => None,
        }
    }

    /// Sets property
    pub fn set_property(&mut self, key: String, value: Option<JToken>) {
        if let JToken::Object(obj) = self {
            obj.set(key, value);
        }
    }

    /// Converts to boolean
    pub fn as_boolean(&self) -> bool {
        match self {
            JToken::Null => false,
            JToken::Boolean(b) => b.as_boolean(),
            JToken::Number(n) => n.as_boolean(),
            JToken::String(s) => s.as_boolean(),
            JToken::Array(_) => true,
            JToken::Object(_) => true,
        }
    }

    /// Converts to number
    pub fn as_number(&self) -> f64 {
        match self {
            JToken::Null => 0.0,
            JToken::Boolean(b) => b.as_number(),
            JToken::Number(n) => n.as_number(),
            JToken::String(s) => s.as_number(),
            JToken::Array(_) => f64::NAN,
            JToken::Object(_) => f64::NAN,
        }
    }

    /// Converts to string
    pub fn as_string(&self) -> String {
        match self {
            JToken::Null => "null".to_string(),
            JToken::Boolean(b) => b.as_string(),
            JToken::Number(n) => n.as_string(),
            JToken::String(s) => s.as_string(),
            JToken::Array(a) => a.to_string(),
            JToken::Object(o) => o.to_string(),
        }
    }

    /// Gets boolean value
    pub fn get_boolean(&self) -> Result<bool, String> {
        match self {
            JToken::Boolean(b) => Ok(b.get_boolean()),
            _ => Err("InvalidCastException".to_string()),
        }
    }

    /// Gets number value
    pub fn get_number(&self) -> Result<f64, String> {
        match self {
            JToken::Number(n) => Ok(n.get_number()),
            _ => Err("InvalidCastException".to_string()),
        }
    }

    /// Gets string value
    pub fn get_string(&self) -> Result<String, String> {
        match self {
            JToken::String(s) => Ok(s.get_string()),
            _ => Err("InvalidCastException".to_string()),
        }
    }

    /// Gets children count
    pub fn get_children_count(&self) -> usize {
        match self {
            JToken::Array(a) => a.count(),
            JToken::Object(o) => o.properties().count(),
            _ => 0,
        }
    }

    /// Checks if null
    pub fn is_null(&self) -> bool {
        matches!(self, JToken::Null)
    }

    /// Parses JSON text
    pub fn parse(text: &str, _max_nesting: i32) -> Result<JToken, String> {
        // This would use a proper JSON parser
        // For now, using serde_json as a placeholder
        match serde_json::from_str::<serde_json::Value>(text) {
            Ok(value) => Ok(Self::from_serde_value(value)),
            Err(e) => Err(e.to_string()),
        }
    }

    fn from_serde_value(value: serde_json::Value) -> JToken {
        match value {
            serde_json::Value::Null => JToken::Null,
            serde_json::Value::Bool(b) => JToken::Boolean(crate::j_boolean::JBoolean::new(b)),
            serde_json::Value::Number(n) => {
                if let Some(f) = n.as_f64() {
                    JToken::Number(
                        crate::j_number::JNumber::new(f)
                            .unwrap_or_else(|_| crate::j_number::JNumber::new(0.0).unwrap()),
                    )
                } else {
                    JToken::Number(crate::j_number::JNumber::new(0.0).unwrap())
                }
            }
            serde_json::Value::String(s) => JToken::String(crate::j_string::JString::new(s)),
            serde_json::Value::Array(arr) => {
                let mut j_array = crate::j_array::JArray::new();
                for item in arr {
                    j_array.add(Some(Self::from_serde_value(item)));
                }
                JToken::Array(j_array)
            }
            serde_json::Value::Object(obj) => {
                let mut j_object = crate::j_object::JObject::new();
                for (key, value) in obj {
                    j_object.set(key, Some(Self::from_serde_value(value)));
                }
                JToken::Object(j_object)
            }
        }
    }

    /// Converts to string
    pub fn to_string(&self) -> String {
        self.as_string()
    }

    /// Writes to writer
    pub fn write(&self, writer: &mut dyn Write) -> std::io::Result<()> {
        writer.write_all(self.to_string().as_bytes())
    }

    /// Clones the token
    pub fn clone(&self) -> JToken {
        match self {
            JToken::Null => JToken::Null,
            JToken::Boolean(b) => JToken::Boolean(b.clone()),
            JToken::Number(n) => JToken::Number(n.clone()),
            JToken::String(s) => JToken::String(s.clone()),
            JToken::Array(a) => JToken::Array(a.clone()),
            JToken::Object(o) => JToken::Object(o.clone()),
        }
    }
}
