//! `JArray` - Rust port of Neo.Json.JArray

use std::fmt;
use std::iter::FromIterator;

use crate::j_token::JToken;
use crate::JsonError;

/// Represents a JSON array (matches the behaviour of the C# implementation).
#[derive(Clone, Debug, PartialEq)]
pub struct JArray {
    items: Vec<Option<JToken>>,
}

impl JArray {
    /// Creates an empty array.
    #[must_use]
    pub const fn new() -> Self {
        Self { items: Vec::new() }
    }

    /// Creates an array from the provided items.
    #[must_use]
    pub const fn from_vec(items: Vec<Option<JToken>>) -> Self {
        Self { items }
    }

    /// Number of elements in the array.
    #[must_use]
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns `true` when the array has no elements.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Alias for `len()` to preserve C# API naming.
    #[must_use]
    pub fn count(&self) -> usize {
        self.len()
    }

    /// Returns the element at `index` without bounds checking.
    #[must_use]
    pub fn get(&self, index: usize) -> Option<&JToken> {
        self.items.get(index).and_then(|value| value.as_ref())
    }

    /// Returns the element at `index` while enforcing bounds checking.
    ///
    /// # Errors
    ///
    /// Returns `JsonError::IndexOutOfRange` if the index is out of bounds.
    pub fn get_checked(&self, index: usize) -> Result<Option<&JToken>, JsonError> {
        if index >= self.items.len() {
            return Err(JsonError::IndexOutOfRange(index));
        }
        Ok(self.items[index].as_ref())
    }

    /// Replaces the element at `index`.
    ///
    /// # Errors
    ///
    /// Returns `JsonError::IndexOutOfRange` if the index is out of bounds.
    pub fn set(&mut self, index: usize, value: Option<JToken>) -> Result<(), JsonError> {
        if index >= self.items.len() {
            return Err(JsonError::IndexOutOfRange(index));
        }
        self.items[index] = value;
        Ok(())
    }

    /// Returns a slice of the underlying storage.
    #[must_use]
    pub fn children(&self) -> &[Option<JToken>] {
        &self.items
    }

    /// Adds a new element to the end of the array.
    pub fn add(&mut self, item: Option<JToken>) {
        self.items.push(item);
    }

    /// Returns a mutable reference to the element at `index`.
    pub fn get_mut(&mut self, index: usize) -> Option<&mut Option<JToken>> {
        self.items.get_mut(index)
    }

    /// Inserts a new element at the specified position.
    ///
    /// # Errors
    ///
    /// Returns `JsonError::IndexOutOfRange` if the index is out of bounds.
    pub fn insert(&mut self, index: usize, item: Option<JToken>) -> Result<(), JsonError> {
        if index > self.items.len() {
            return Err(JsonError::IndexOutOfRange(index));
        }
        self.items.insert(index, item);
        Ok(())
    }

    /// Removes the first occurrence of `item`.
    pub fn remove(&mut self, item: &Option<JToken>) -> bool {
        if let Some(position) = self.index_of(item) {
            self.items.remove(position);
            true
        } else {
            false
        }
    }

    /// Removes the element at `index`.
    ///
    /// # Errors
    ///
    /// Returns `JsonError::IndexOutOfRange` if the index is out of bounds.
    pub fn remove_at(&mut self, index: usize) -> Result<(), JsonError> {
        if index >= self.items.len() {
            return Err(JsonError::IndexOutOfRange(index));
        }
        self.items.remove(index);
        Ok(())
    }

    /// Clears the array.
    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Returns `true` if the array contains `item`.
    #[must_use]
    pub fn contains(&self, item: &Option<JToken>) -> bool {
        self.items.iter().any(|candidate| candidate == item)
    }

    /// Returns the index of `item` or `None` when not found.
    #[must_use]
    pub fn index_of(&self, item: &Option<JToken>) -> Option<usize> {
        self.items.iter().position(|candidate| candidate == item)
    }

    /// Iterator over the stored items.
    pub fn iter(&self) -> impl Iterator<Item = &Option<JToken>> {
        self.items.iter()
    }

    /// Mutable iterator over the stored items.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = &mut Option<JToken>> {
        self.items.iter_mut()
    }
}

impl Default for JArray {
    fn default() -> Self {
        Self::new()
    }
}

impl FromIterator<Option<JToken>> for JArray {
    fn from_iter<T: IntoIterator<Item = Option<JToken>>>(iter: T) -> Self {
        Self::from_vec(iter.into_iter().collect())
    }
}

impl FromIterator<JToken> for JArray {
    fn from_iter<T: IntoIterator<Item = JToken>>(iter: T) -> Self {
        let items = iter.into_iter().map(Some).collect();
        Self::from_vec(items)
    }
}

impl fmt::Display for JArray {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("[")?;
        for (index, item) in self.items.iter().enumerate() {
            if index > 0 {
                f.write_str(",")?;
            }
            match item {
                Some(token) => write!(f, "{token}")?,
                None => f.write_str("null")?,
            }
        }
        f.write_str("]")
    }
}

impl From<Vec<Option<JToken>>> for JArray {
    fn from(value: Vec<Option<JToken>>) -> Self {
        Self::from_vec(value)
    }
}

impl From<Vec<JToken>> for JArray {
    fn from(value: Vec<JToken>) -> Self {
        let items = value.into_iter().map(Some).collect();
        Self::from_vec(items)
    }
}

impl IntoIterator for JArray {
    type Item = Option<JToken>;
    type IntoIter = std::vec::IntoIter<Option<JToken>>;

    fn into_iter(self) -> Self::IntoIter {
        self.items.into_iter()
    }
}
