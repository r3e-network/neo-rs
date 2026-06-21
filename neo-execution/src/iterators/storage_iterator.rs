//! StorageIterator - matches C# Neo.SmartContract.Iterators.StorageIterator exactly

use crate::iterators::iterator::StorageIterator as Iter;
use neo_error::{CoreError, CoreResult};
use neo_primitives::FindOptions;
use neo_serialization::binary_serializer::BinarySerializer;
use neo_storage::StorageItem;
use neo_storage::StorageKey;
use neo_vm::StackItem;
use neo_vm_rs::{ExecutionEngineLimits, StackValue};

/// Storage iterator for iterating over storage items (matches C# StorageIterator)
#[derive(Debug)]
pub struct StorageIterator {
    /// The underlying enumerator
    items: Vec<(StorageKey, StorageItem)>,
    /// Current position
    current: Option<usize>,
    /// Prefix length to remove
    prefix_length: usize,
    /// Find options
    options: FindOptions,
}

fn stack_value_to_stack_item(value: StackValue) -> CoreResult<StackItem> {
    StackItem::try_from(value).map_err(|error| CoreError::other(error.to_string()))
}

impl StorageIterator {
    /// Creates a new storage iterator
    pub fn new(
        items: Vec<(StorageKey, StorageItem)>,
        prefix_length: usize,
        options: FindOptions,
    ) -> Self {
        Self {
            items,
            current: None,
            prefix_length,
            options,
        }
    }

    /// Creates from an iterator
    pub fn from_iter<I>(iter: I, prefix_length: usize, options: FindOptions) -> Self
    where
        I: Iterator<Item = (StorageKey, StorageItem)>,
    {
        Self::new(iter.collect(), prefix_length, options)
    }
}

impl Iter for StorageIterator {
    fn next(&mut self) -> bool {
        match self.current {
            None => {
                if !self.items.is_empty() {
                    self.current = Some(0);
                    true
                } else {
                    false
                }
            }
            Some(idx) => {
                if idx + 1 < self.items.len() {
                    self.current = Some(idx + 1);
                    true
                } else {
                    false
                }
            }
        }
    }

    fn value(&self) -> CoreResult<StackItem> {
        let idx = match self.current {
            Some(i) if i < self.items.len() => i,
            _ => {
                return Err(CoreError::invalid_operation(
                    "Iterator is not positioned on an element",
                ));
            }
        };

        let (key, value) = &self.items[idx];

        // Get key bytes
        let mut key_bytes = key.suffix().to_vec();

        // Remove prefix if requested
        if self.options.contains(FindOptions::RemovePrefix) && key_bytes.len() >= self.prefix_length
        {
            key_bytes = key_bytes[self.prefix_length..].to_vec();
        }

        // Get value
        let raw_value = value.value_bytes();
        let limits = ExecutionEngineLimits::default();
        let mut value_item = if self.options.contains(FindOptions::DeserializeValues) {
            BinarySerializer::deserialize_stack_value_with_limits(
                &raw_value,
                limits.max_item_size as usize,
                limits.max_stack_size as usize,
            )?
        } else {
            StackValue::ByteString(raw_value.to_vec())
        };

        // Pick field if requested
        if self.options.contains(FindOptions::PickField0) {
            value_item = match value_item {
                StackValue::Array(0, array) | StackValue::Struct(0, array) => array
                    .first()
                    .cloned()
                    .ok_or_else(|| CoreError::invalid_operation("PickField0 requires field 0"))?,
                _ => {
                    return Err(CoreError::invalid_operation(
                        "PickField0 requires an array value",
                    ));
                }
            };
        } else if self.options.contains(FindOptions::PickField1) {
            value_item = match value_item {
                StackValue::Array(0, array) | StackValue::Struct(0, array) => array
                    .get(1)
                    .cloned()
                    .ok_or_else(|| CoreError::invalid_operation("PickField1 requires field 1"))?,
                _ => {
                    return Err(CoreError::invalid_operation(
                        "PickField1 requires an array value",
                    ));
                }
            };
        }

        // Return based on options
        if self.options.contains(FindOptions::KeysOnly) {
            stack_value_to_stack_item(StackValue::ByteString(key_bytes))
        } else if self.options.contains(FindOptions::ValuesOnly) {
            stack_value_to_stack_item(value_item)
        } else {
            // Return struct with key and value
            stack_value_to_stack_item(StackValue::Struct(
                0,
                vec![StackValue::ByteString(key_bytes), value_item],
            ))
        }
    }

    fn dispose(&mut self) {
        self.items.clear();
        self.current = None;
    }
}

#[cfg(test)]
#[path = "../tests/iterators/storage_iterator.rs"]
mod tests;
