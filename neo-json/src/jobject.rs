use std::collections::BTreeMap;
use serde_json::Value as JsonValue;

/// Represents a JSON object.
#[derive(Clone, Debug)]
pub struct JObject {
    properties: BTreeMap<String, Option<JToken>>,
}

impl JObject {
    /// Creates a new empty JObject.
    pub fn new() -> Self {
        JObject {
            properties: BTreeMap::new(),
        }
    }

    /// Gets or sets the properties of the JSON object.
    pub fn properties(&self) -> &BTreeMap<String, Option<JToken>> {
        &self.properties
    }

    /// Gets or sets the properties of the JSON object.
    pub fn properties_mut(&mut self) -> &mut BTreeMap<String, Option<JToken>> {
        &mut self.properties
    }

    /// Gets a property value by name.
    pub fn get(&self, name: &str) -> Option<&Option<JToken>> {
        self.properties.get(name)
    }

    /// Sets a property value by name.
    pub fn set(&mut self, name: String, value: Option<JToken>) {
        self.properties.insert(name, value);
    }

    /// Determines whether the JSON object contains a property with the specified name.
    pub fn contains_property(&self, key: &str) -> bool {
        self.properties.contains_key(key)
    }

    /// Clears all properties from the JSON object.
    pub fn clear(&mut self) {
        self.properties.clear();
    }

    /// Writes the JSON object to a JsonValue.
    pub fn to_json_value(&self) -> JsonValue {
        let mut map = serde_json::Map::new();
        for (key, value) in &self.properties {
            map.insert(key.clone(), match value {
                Some(token) => token.to_json_value(),
                None => JsonValue::Null,
            });
        }
        JsonValue::Object(map)
    }

    /// Creates a deep copy of the current JSON object.
    pub fn clone(&self) -> Self {
        JObject {
            properties: self.properties.iter().map(|(k, v)| {
                (k.clone(), v.as_ref().map(|token| token.clone()))
            }).collect(),
        }
    }
}

impl Default for JObject {
    fn default() -> Self {
        Self::new()
    }
}

/// Represents a JSON token.
#[derive(Clone, Debug)]
pub enum JToken {
    Object(JObject),
    Array(Vec<Option<JToken>>),
    String(String),
    Number(f64),
    Boolean(bool),
    Null,
}

impl JToken {
    /// Converts the JToken to a JsonValue.
    pub fn to_json_value(&self) -> JsonValue {
        match self {
            JToken::Object(obj) => obj.to_json_value(),
            JToken::Array(arr) => JsonValue::Array(
                arr.iter().map(|item| match item {
                    Some(token) => token.to_json_value(),
                    None => JsonValue::Null,
                }).collect()
            ),
            JToken::String(s) => JsonValue::String(s.clone()),
            JToken::Number(n) => JsonValue::Number(serde_json::Number::from_f64(*n).unwrap_or(serde_json::Number::from(0))),
            JToken::Boolean(b) => JsonValue::Bool(*b),
            JToken::Null => JsonValue::Null,
        }
    }

    /// Creates a deep copy of the current JSON token.
    pub fn clone(&self) -> Self {
        match self {
            JToken::Object(obj) => JToken::Object(obj.clone()),
            JToken::Array(arr) => JToken::Array(arr.iter().map(|item| item.as_ref().map(|token| token.clone())).collect()),
            JToken::String(s) => JToken::String(s.clone()),
            JToken::Number(n) => JToken::Number(*n),
            JToken::Boolean(b) => JToken::Boolean(*b),
            JToken::Null => JToken::Null,
        }
    }
}
