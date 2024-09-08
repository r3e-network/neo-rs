use std::iter::Iterator;
use NeoRust::types::StackItem;
use neo_vm::reference_counter::ReferenceCounter;
use crate::neo_contract::find_options::FindOptions;
use crate::neo_contract::storage_item::StorageItem;
use crate::neo_contract::storage_key::StorageKey;

/// Represents an iterator over storage items in a Neo smart contract.
pub struct StorageIterator {
    enumerator: Box<dyn Iterator<Item = (StorageKey, StorageItem)>>,
    prefix_length: usize,
    options: FindOptions,
}

impl StorageIterator {
    /// Creates a new `StorageIterator`.
    ///
    /// # Arguments
    ///
    /// * `enumerator` - An iterator over storage key-value pairs.
    /// * `prefix_length` - The length of the prefix to be removed, if applicable.
    /// * `options` - Options for customizing the iterator's behavior.
    pub fn new(
        enumerator: Box<dyn Iterator<Item = (StorageKey, StorageItem)>>,
        prefix_length: usize,
        options: FindOptions,
    ) -> Self {
        Self {
            enumerator,
            prefix_length,
            options,
        }
    }
}

impl Iterator for StorageIterator {
    type Item = ();

    fn next(&mut self) -> Option<Self::Item> {
        self.enumerator.next()
    }
}

impl IIterator for StorageIterator {
    fn value(&self, reference_counter: &mut ReferenceCounter) -> StackItem {
        let (key, value) = self.enumerator.peek().unwrap();
        let mut key_bytes = key.as_bytes();
        let value_bytes = value.as_bytes();

        if self.options.contains(FindOptions::RemovePrefix) {
            key_bytes = &key_bytes[self.prefix_length..];
        }

        let item = if self.options.contains(FindOptions::DeserializeValues) {
            BinaryReader::deserialize_stack_item(value_bytes, ExecutionEngineLimits::default(), reference_counter)
                .unwrap_or_else(|_| StackItem::ByteString(value_bytes.to_vec()))
        } else {
            StackItem::ByteString(value_bytes.to_vec())
        };

        let item = if self.options.contains(FindOptions::PickField0) {
            if let StackItem::Array(array) = item {
                array.get(0).cloned().unwrap_or(StackItem::Null)
            } else {
                item
            }
        } else if self.options.contains(FindOptions::PickField1) {
            if let StackItem::Array(array) = item {
                array.get(1).cloned().unwrap_or(StackItem::Null)
            } else {
                item
            }
        } else {
            item
        };

        if self.options.contains(FindOptions::KeysOnly) {
            StackItem::ByteString(key_bytes.to_vec())
        } else if self.options.contains(FindOptions::ValuesOnly) {
            item
        } else {
            StackItem::Struct(Struct::new(vec![
                StackItem::ByteString(key_bytes.to_vec()),
                item,
            ]))
        }
    }
}
