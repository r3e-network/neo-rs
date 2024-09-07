use std::collections::VecDeque;
use serde_json::Value as JsonValue;
use crate::jtoken::JToken;

/// Represents a JSON array.
#[derive(Clone, Debug)]
pub struct JArray {
    items: VecDeque<Option<JToken>>,
}

impl JArray {
    /// Creates a new `JArray` instance.
    ///
    /// # Arguments
    ///
    /// * `items` - The initial items in the array.
    pub fn new(items: impl IntoIterator<Item = Option<JToken>>) -> Self {
        Self {
            items: items.into_iter().collect(),
        }
    }

    /// Returns a reference to the item at the specified index.
    pub fn get(&self, index: usize) -> Option<&Option<JToken>> {
        self.items.get(index)
    }

    /// Returns a mutable reference to the item at the specified index.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut Option<JToken>> {
        self.items.get_mut(index)
    }

    /// Returns an iterator over the items in the array.
    pub fn iter(&self) -> impl Iterator<Item = &Option<JToken>> {
        self.items.iter()
    }

    /// Returns a mutable iterator over the items in the array.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Option<JToken>> {
        self.items.iter_mut()
    }

    /// Adds an item to the end of the array.
    pub fn push(&mut self, item: Option<JToken>) {
        self.items.push_back(item);
    }

    /// Removes and returns the last item in the array.
    pub fn pop(&mut self) -> Option<Option<JToken>> {
        self.items.pop_back()
    }

    /// Returns the number of items in the array.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns `true` if the array is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Removes all items from the array.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Returns `true` if the array contains the specified item.
    pub fn contains(&self, item: &Option<JToken>) -> bool {
        self.items.contains(item)
    }

    /// Returns the index of the first occurrence of the specified item.
    pub fn index_of(&self, item: &Option<JToken>) -> Option<usize> {
        self.items.iter().position(|x| x == item)
    }

    /// Inserts an item at the specified index.
    pub fn insert(&mut self, index: usize, item: Option<JToken>) {
        self.items.insert(index, item);
    }

    /// Removes the item at the specified index.
    pub fn remove(&mut self, index: usize) -> Option<Option<JToken>> {
        if index < self.items.len() {
            Some(self.items.remove(index).unwrap())
        } else {
            None
        }
    }

    /// Converts the `JArray` to a JSON string.
    pub fn to_string(&self) -> String {
        serde_json::to_string(&self.to_json_value()).unwrap()
    }

    /// Converts the `JArray` to a `serde_json::Value`.
    pub fn to_json_value(&self) -> JsonValue {
        JsonValue::Array(
            self.items
                .iter()
                .map(|item| match item {
                    Some(token) => token.to_json_value(),
                    None => JsonValue::Null,
                })
                .collect(),
        )
    }

    /// Creates a deep clone of the `JArray`.
    pub fn clone(&self) -> Self {
        Self {
            items: self.items.iter().map(|item| item.as_ref().map(JToken::clone)).collect(),
        }
    }
}

impl From<Vec<Option<JToken>>> for JArray {
    fn from(value: Vec<Option<JToken>>) -> Self {
        Self::new(value)
    }
}

// Note: The `JToken` struct and its implementation are not provided in this snippet.
// You would need to implement `JToken` separately, including methods like `to_json_value` and `clone`.
