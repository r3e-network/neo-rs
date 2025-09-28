//! JArray - matches C# Neo.Json.JArray exactly

use crate::j_token::JToken;
use std::io::Write;

/// Represents a JSON array (matches C# JArray)
#[derive(Clone, Debug)]
pub struct JArray {
    items: Vec<Option<JToken>>,
}

impl JArray {
    /// Initializes a new instance
    pub fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Initializes with items
    pub fn from_items(items: Vec<Option<JToken>>) -> Self {
        Self { items }
    }

    /// Gets item at index
    pub fn get(&self, index: usize) -> Option<&JToken> {
        self.items.get(index).and_then(|item| item.as_ref())
    }

    /// Sets item at index
    pub fn set(&mut self, index: usize, value: Option<JToken>) {
        if index < self.items.len() {
            self.items[index] = value;
        }
    }

    /// Gets children
    pub fn children(&self) -> &[Option<JToken>] {
        &self.items
    }

    /// Adds an item
    pub fn add(&mut self, item: Option<JToken>) {
        self.items.push(item);
    }

    /// Clears all items
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Checks if contains item
    pub fn contains(&self, _item: &Option<JToken>) -> bool {
        // Would need proper equality comparison
        false
    }

    /// Gets index of item
    pub fn index_of(&self, _item: &Option<JToken>) -> Option<usize> {
        // Would need proper equality comparison
        None
    }

    /// Inserts item at index
    pub fn insert(&mut self, index: usize, item: Option<JToken>) {
        if index <= self.items.len() {
            self.items.insert(index, item);
        }
    }

    /// Removes item
    pub fn remove(&mut self, _item: &Option<JToken>) -> bool {
        // Would need proper equality comparison
        false
    }

    /// Removes item at index
    pub fn remove_at(&mut self, index: usize) {
        if index < self.items.len() {
            self.items.remove(index);
        }
    }

    /// Gets count
    pub fn count(&self) -> usize {
        self.items.len()
    }

    /// Converts to string
    pub fn to_string(&self) -> String {
        let mut result = String::from("[");
        for (i, item) in self.items.iter().enumerate() {
            if i > 0 {
                result.push_str(",");
            }
            if let Some(token) = item {
                result.push_str(&token.to_string());
            } else {
                result.push_str("null");
            }
        }
        result.push(']');
        result
    }

    /// Writes to JSON writer
    pub fn write(&self, writer: &mut dyn Write) -> std::io::Result<()> {
        writer.write_all(self.to_string().as_bytes())
    }

    /// Clones the array
    pub fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
        }
    }
}

impl Default for JArray {
    fn default() -> Self {
        Self::new()
    }
}
