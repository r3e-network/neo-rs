//! StorageIterator - matches C# Neo.SmartContract.Iterators.StorageIterator exactly

use crate::smart_contract::binary_serializer::BinarySerializer;
use crate::smart_contract::find_options::FindOptions;
use crate::smart_contract::iterators::i_iterator::IIterator;
use crate::smart_contract::storage_item::StorageItem;
use crate::smart_contract::storage_key::StorageKey;
use neo_vm::{execution_engine_limits::ExecutionEngineLimits, StackItem};

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

impl IIterator for StorageIterator {
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

    fn value(&self) -> StackItem {
        let idx = match self.current {
            Some(i) if i < self.items.len() => i,
            _ => return StackItem::Null,
        };

        let (key, value) = &self.items[idx];

        // Get key bytes
        let mut key_bytes = key.key().to_vec();

        // Remove prefix if requested
        if self.options.contains(FindOptions::RemovePrefix) && key_bytes.len() >= self.prefix_length
        {
            key_bytes = key_bytes[self.prefix_length..].to_vec();
        }

        // Get value
        let raw_value = value.get_value();
        let mut value_item = if self.options.contains(FindOptions::DeserializeValues) {
            match BinarySerializer::deserialize(&raw_value, &ExecutionEngineLimits::default(), None)
            {
                Ok(item) => item,
                Err(_) => StackItem::from_byte_string(raw_value.clone()),
            }
        } else {
            StackItem::from_byte_string(raw_value.clone())
        };

        // Pick field if requested
        if self.options.contains(FindOptions::PickField0) {
            value_item = match value_item {
                StackItem::Array(array) => array
                    .items()
                    .get(0)
                    .cloned()
                    .unwrap_or_else(StackItem::null),
                StackItem::Struct(struct_item) => struct_item
                    .items()
                    .get(0)
                    .cloned()
                    .unwrap_or_else(StackItem::null),
                other => other,
            };
        } else if self.options.contains(FindOptions::PickField1) {
            value_item = match value_item {
                StackItem::Array(array) => array
                    .items()
                    .get(1)
                    .cloned()
                    .unwrap_or_else(StackItem::null),
                StackItem::Struct(struct_item) => struct_item
                    .items()
                    .get(1)
                    .cloned()
                    .unwrap_or_else(StackItem::null),
                other => other,
            };
        }

        // Return based on options
        if self.options.contains(FindOptions::KeysOnly) {
            StackItem::from_byte_string(key_bytes)
        } else if self.options.contains(FindOptions::ValuesOnly) {
            value_item
        } else {
            // Return struct with key and value
            StackItem::from_struct(vec![StackItem::from_byte_string(key_bytes), value_item])
        }
    }

    fn dispose(&mut self) {
        self.items.clear();
        self.current = None;
    }
}
