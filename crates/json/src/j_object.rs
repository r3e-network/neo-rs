//! JObject - matches C# Neo.Json.JObject exactly

use crate::j_token::JToken;
use crate::ordered_dictionary::OrderedDictionary;
use std::io::Write;

/// Represents a JSON object (matches C# JObject)
#[derive(Clone, Debug)]
pub struct JObject {
    properties: OrderedDictionary<String, Option<JToken>>,
}

impl JObject {
    /// Initializes a new instance
    pub fn new() -> Self {
        Self {
            properties: OrderedDictionary::new(),
        }
    }

    /// Gets property by name
    pub fn get(&self, name: &str) -> Option<&JToken> {
        self.properties.get(name).and_then(|v| v.as_ref())
    }

    /// Sets property
    pub fn set(&mut self, name: String, value: Option<JToken>) {
        self.properties.insert(name, value);
    }

    /// Gets properties
    pub fn properties(&self) -> &OrderedDictionary<String, Option<JToken>> {
        &self.properties
    }

    /// Gets mutable properties
    pub fn properties_mut(&mut self) -> &mut OrderedDictionary<String, Option<JToken>> {
        &mut self.properties
    }

    /// Gets children (values)
    pub fn children(&self) -> Vec<&Option<JToken>> {
        self.properties.values()
    }

    /// Checks if contains property
    pub fn contains_property(&self, key: &str) -> bool {
        self.properties.contains_key(key)
    }

    /// Clears all properties
    pub fn clear(&mut self) {
        self.properties.clear();
    }

    /// Converts to string
    pub fn to_string(&self) -> String {
        let mut result = String::from("{");
        let mut first = true;
        for (key, value) in self.properties.iter() {
            if !first {
                result.push(',');
            }
            first = false;
            result.push('"');
            result.push_str(key);
            result.push_str("\":");
            if let Some(token) = value {
                result.push_str(&token.to_string());
            } else {
                result.push_str("null");
            }
        }
        result.push('}');
        result
    }

    /// Writes to JSON writer
    pub fn write(&self, writer: &mut dyn Write) -> std::io::Result<()> {
        writer.write_all(self.to_string().as_bytes())
    }

    /// Clones the object
    pub fn clone(&self) -> Self {
        Self {
            properties: self.properties.clone(),
        }
    }
}

impl Default for JObject {
    fn default() -> Self {
        Self::new()
    }
}
