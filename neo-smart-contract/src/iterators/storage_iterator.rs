//! StorageIterator - matches C# Neo.SmartContract.Iterators.StorageIterator exactly

use neo_primitives::FindOptions;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::binary_serializer::BinarySerializer;
use neo_vm::{ExecutionEngineLimits, StackItem};

use crate::iterators::iterator::StorageIterator as Iter;

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

    pub(crate) fn trace_position(&self) -> (Option<usize>, usize, Option<Vec<u8>>) {
        (
            self.current,
            self.items.len(),
            self.current
                .and_then(|index| self.items.get(index))
                .map(|(key, _)| key.suffix().to_vec()),
        )
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

    fn value(&self) -> StackItem {
        let idx = match self.current {
            Some(i) if i < self.items.len() => i,
            _ => return StackItem::Null,
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
                    .first()
                    .cloned()
                    .unwrap_or_else(StackItem::null),
                StackItem::Struct(struct_item) => struct_item
                    .items()
                    .first()
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

#[cfg(test)]
mod tests {
    use super::*;
    use num_bigint::BigInt;

    fn create_test_items(count: usize) -> Vec<(StorageKey, StorageItem)> {
        (0..count)
            .map(|i| {
                let mut key_data = vec![0xAA, 0xBB]; // prefix
                key_data.push(i as u8);
                let key = StorageKey::new(1, key_data);
                let mut item = StorageItem::new();
                item.deserialize_from_bytes(&[i as u8]);
                (key, item)
            })
            .collect()
    }

    #[test]
    fn new_creates_iterator_at_start() {
        let items = create_test_items(3);
        let iter = StorageIterator::new(items, 0, FindOptions::None);
        assert!(iter.current.is_none());
    }

    #[test]
    fn next_advances_to_first_item() {
        let items = create_test_items(3);
        let mut iter = StorageIterator::new(items, 0, FindOptions::None);
        assert!(iter.next());
        assert_eq!(iter.current, Some(0));
    }

    #[test]
    fn next_advances_through_all_items() {
        let items = create_test_items(3);
        let mut iter = StorageIterator::new(items, 0, FindOptions::None);

        assert!(iter.next()); // 0
        assert!(iter.next()); // 1
        assert!(iter.next()); // 2
        assert!(!iter.next()); // end
    }

    #[test]
    fn next_returns_false_for_empty_iterator() {
        let iter = StorageIterator::new(vec![], 0, FindOptions::None);
        let mut iter = iter;
        assert!(!iter.next());
    }

    #[test]
    fn value_returns_null_before_first_next() {
        let items = create_test_items(3);
        let iter = StorageIterator::new(items, 0, FindOptions::None);
        assert_eq!(iter.value(), StackItem::Null);
    }

    #[test]
    fn value_returns_struct_with_key_and_value_by_default() {
        let items = create_test_items(1);
        let mut iter = StorageIterator::new(items, 0, FindOptions::None);
        iter.next();

        let result = iter.value();
        if let StackItem::Struct(s) = result {
            assert_eq!(s.items().len(), 2);
        } else {
            panic!("Expected Struct");
        }
    }

    #[test]
    fn keys_only_option_returns_only_key() {
        let items = create_test_items(1);
        let mut iter = StorageIterator::new(items, 0, FindOptions::KeysOnly);
        iter.next();

        let result = iter.value();
        assert!(matches!(result, StackItem::ByteString(_)));
    }

    #[test]
    fn values_only_option_returns_only_value() {
        let items = create_test_items(1);
        let mut iter = StorageIterator::new(items, 0, FindOptions::ValuesOnly);
        iter.next();

        let result = iter.value();
        assert!(matches!(result, StackItem::ByteString(_)));
    }

    #[test]
    fn remove_prefix_option_strips_prefix_bytes() {
        let items = create_test_items(1);
        let prefix_len = 2;
        let mut iter = StorageIterator::new(
            items,
            prefix_len,
            FindOptions::KeysOnly | FindOptions::RemovePrefix,
        );
        iter.next();

        if let StackItem::ByteString(bytes) = iter.value() {
            // Should only have the suffix byte, prefix removed
            assert_eq!(bytes.len(), 1);
        } else {
            panic!("Expected ByteString");
        }
    }

    #[test]
    fn deserialize_values_option_deserializes_binary_data() {
        let key = StorageKey::new(1, vec![0xAA]);
        let mut item = StorageItem::new();

        // Serialize an array as the value
        let array = StackItem::from_array(vec![
            StackItem::Integer(42.into()),
            StackItem::Boolean(true),
        ]);
        let serialized = BinarySerializer::serialize(&array, &ExecutionEngineLimits::default())
            .expect("serialize");
        item.deserialize_from_bytes(&serialized);

        let items = vec![(key, item)];
        let mut iter = StorageIterator::new(
            items,
            0,
            FindOptions::ValuesOnly | FindOptions::DeserializeValues,
        );
        iter.next();

        let result = iter.value();
        assert!(matches!(result, StackItem::Array(_)));
    }

    #[test]
    fn pick_field0_extracts_first_field() {
        let key = StorageKey::new(1, vec![0xAA]);
        let mut item = StorageItem::new();

        let array = StackItem::from_array(vec![
            StackItem::Integer(42.into()),
            StackItem::Integer(99.into()),
        ]);
        let serialized = BinarySerializer::serialize(&array, &ExecutionEngineLimits::default())
            .expect("serialize");
        item.deserialize_from_bytes(&serialized);

        let items = vec![(key, item)];
        let mut iter = StorageIterator::new(
            items,
            0,
            FindOptions::ValuesOnly | FindOptions::DeserializeValues | FindOptions::PickField0,
        );
        iter.next();

        let result = iter.value();
        // Just verify we got an Integer back
        assert!(matches!(result, StackItem::Integer(_)));
    }

    #[test]
    fn pick_field1_extracts_second_field() {
        let key = StorageKey::new(1, vec![0xAA]);
        let mut item = StorageItem::new();

        let array = StackItem::from_array(vec![
            StackItem::Integer(42.into()),
            StackItem::Integer(99.into()),
        ]);
        let serialized = BinarySerializer::serialize(&array, &ExecutionEngineLimits::default())
            .expect("serialize");
        item.deserialize_from_bytes(&serialized);

        let items = vec![(key, item)];
        let mut iter = StorageIterator::new(
            items,
            0,
            FindOptions::ValuesOnly | FindOptions::DeserializeValues | FindOptions::PickField1,
        );
        iter.next();

        let result = iter.value();
        // Just verify we got an Integer back
        assert!(matches!(result, StackItem::Integer(_)));
    }

    #[test]
    fn dispose_clears_items_and_resets_position() {
        let items = create_test_items(3);
        let mut iter = StorageIterator::new(items, 0, FindOptions::None);
        iter.next();

        iter.dispose();
        assert!(iter.current.is_none());
        assert!(iter.items.is_empty());
    }

    #[test]
    fn from_iter_creates_iterator_from_iterator() {
        let items = create_test_items(3);
        let iter = StorageIterator::from_iter(items.into_iter(), 0, FindOptions::None);
        assert_eq!(iter.items.len(), 3);
    }

    #[test]
    fn multiple_next_calls_after_end_stay_at_end() {
        let items = create_test_items(1);
        let mut iter = StorageIterator::new(items, 0, FindOptions::None);

        assert!(iter.next());
        assert!(!iter.next());
        assert!(!iter.next());
        assert!(!iter.next());
    }

    #[test]
    fn value_after_disposal_returns_null() {
        let items = create_test_items(1);
        let mut iter = StorageIterator::new(items, 0, FindOptions::None);
        iter.next();
        iter.dispose();

        assert_eq!(iter.value(), StackItem::Null);
    }

    #[test]
    fn trace_position_reports_correct_state() {
        let items = create_test_items(3);
        let mut iter = StorageIterator::new(items, 0, FindOptions::None);

        let (pos, len, _) = iter.trace_position();
        assert!(pos.is_none());
        assert_eq!(len, 3);

        iter.next();
        let (pos, len, key) = iter.trace_position();
        assert_eq!(pos, Some(0));
        assert_eq!(len, 3);
        assert!(key.is_some());
    }
}
