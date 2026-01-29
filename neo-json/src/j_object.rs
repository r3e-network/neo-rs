//! `JObject` - Rust port of Neo.Json.JObject

use std::fmt;
use std::iter::FromIterator;

use crate::j_token::JToken;
use crate::ordered_dictionary::OrderedDictionary;

/// Represents a JSON object (mirrors the behaviour of the C# implementation).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JObject {
    properties: OrderedDictionary<String, Option<JToken>>,
}

impl JObject {
    /// Creates an empty object.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Builds an object from an ordered dictionary of properties.
    #[must_use]
    pub const fn from_properties(properties: OrderedDictionary<String, Option<JToken>>) -> Self {
        Self { properties }
    }

    /// Returns the underlying ordered dictionary.
    #[must_use]
    pub const fn properties(&self) -> &OrderedDictionary<String, Option<JToken>> {
        &self.properties
    }

    /// Mutable reference to the underlying ordered dictionary.
    pub fn properties_mut(&mut self) -> &mut OrderedDictionary<String, Option<JToken>> {
        &mut self.properties
    }

    /// Returns a property value by name.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&JToken> {
        self.properties.get(name).and_then(|value| value.as_ref())
    }

    /// Sets or replaces the property identified by `name`.
    pub fn set(&mut self, name: String, value: Option<JToken>) {
        self.properties.insert(name, value);
    }

    /// Convenience helper mirroring the older API that accepted a `JToken` directly.
    pub fn insert(&mut self, name: String, value: JToken) {
        self.set(name, Some(value));
    }

    /// Number of stored properties.
    #[must_use]
    pub fn len(&self) -> usize {
        self.properties.count()
    }

    /// Returns `true` when the object has no properties.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Returns the values in insertion order.
    #[must_use]
    pub fn children(&self) -> Vec<&Option<JToken>> {
        self.properties.values().collect()
    }

    /// Returns an iterator over the `(key, value)` pairs.
    pub fn iter(&self) -> impl Iterator<Item = (&String, &Option<JToken>)> {
        self.properties.iter()
    }

    /// Returns `true` if the object contains a property with the supplied name.
    #[must_use]
    pub fn contains_property(&self, key: &str) -> bool {
        self.properties.contains_key(key)
    }

    /// Removes all properties.
    pub fn clear(&mut self) {
        self.properties.clear();
    }
}

impl fmt::Display for JObject {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("{")?;
        for (index, (key, value)) in self.properties.iter().enumerate() {
            if index > 0 {
                f.write_str(",")?;
            }
            write!(f, "\"{key}\":")?;
            match value {
                Some(token) => write!(f, "{token}")?,
                None => f.write_str("null")?,
            }
        }
        f.write_str("}")
    }
}

impl From<OrderedDictionary<String, Option<JToken>>> for JObject {
    fn from(value: OrderedDictionary<String, Option<JToken>>) -> Self {
        Self::from_properties(value)
    }
}

impl FromIterator<(String, Option<JToken>)> for JObject {
    fn from_iter<T: IntoIterator<Item = (String, Option<JToken>)>>(iter: T) -> Self {
        let mut dict = OrderedDictionary::new();
        for (key, value) in iter {
            dict.insert(key, value);
        }
        Self::from_properties(dict)
    }
}
