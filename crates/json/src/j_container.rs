//! JContainer - matches C# Neo.Json.JContainer exactly

use crate::j_array::JArray;
use crate::j_object::JObject;
use crate::j_token::JToken;

/// Abstract container for JSON arrays and objects (matches C# JContainer)
pub trait JContainer {
    /// Gets child at index
    fn get_child_at(&self, index: usize) -> Option<&JToken>;

    /// Gets all children as references
    fn get_children(&self) -> Vec<&Option<JToken>>;

    /// Gets count of children
    fn count(&self) -> usize {
        self.get_children().len()
    }

    /// Clears all children
    fn clear(&mut self);
}

impl JContainer for crate::j_array::JArray {
    fn get_child_at(&self, index: usize) -> Option<&JToken> {
        self.get(index)
    }

    fn get_children(&self) -> Vec<&Option<JToken>> {
        self.children().iter().collect::<Vec<_>>()
    }

    fn clear(&mut self) {
        JArray::clear(self);
    }
}

impl JContainer for crate::j_object::JObject {
    fn get_child_at(&self, _index: usize) -> Option<&JToken> {
        None // Objects don't support index access
    }

    fn get_children(&self) -> Vec<&Option<JToken>> {
        self.children()
    }

    fn clear(&mut self) {
        JObject::clear(self);
    }
}
