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
mod tests {
    use super::*;

    fn iterator_for_value(value: Vec<u8>, options: FindOptions) -> StorageIterator {
        StorageIterator::new(
            vec![(
                StorageKey::new(7, vec![0x01]),
                StorageItem::from_bytes(value),
            )],
            0,
            options,
        )
    }

    #[test]
    fn value_before_next_faults_like_csharp_enumerator_current() {
        let iterator = iterator_for_value(vec![0x01], FindOptions::ValuesOnly);

        assert!(
            iterator.value().is_err(),
            "C# StorageIterator.Value reads enumerator.Current and faults before MoveNext"
        );
    }

    #[test]
    fn deserialize_values_propagates_invalid_storage_payload() {
        let mut iterator = iterator_for_value(
            vec![0xff],
            FindOptions::ValuesOnly | FindOptions::DeserializeValues,
        );

        assert!(iterator.next());
        assert!(
            iterator.value().is_err(),
            "C# BinarySerializer.Deserialize failures are not converted to raw bytes"
        );
    }

    #[test]
    fn pick_field_requires_deserialized_array_like_csharp() {
        let serialized_integer =
            BinarySerializer::serialize(&StackItem::from_i64(1), &ExecutionEngineLimits::default())
                .expect("integer serializes");
        let mut iterator = iterator_for_value(
            serialized_integer,
            FindOptions::ValuesOnly | FindOptions::DeserializeValues | FindOptions::PickField0,
        );

        assert!(iterator.next());
        assert!(
            iterator.value().is_err(),
            "C# casts deserialized values to Array before PickField0/PickField1"
        );
    }

    #[test]
    fn pick_field_out_of_range_faults_like_csharp_array_indexer() {
        let serialized_array = BinarySerializer::serialize(
            &StackItem::from_array(vec![StackItem::from_i64(1)]),
            &ExecutionEngineLimits::default(),
        )
        .expect("array serializes");
        let mut iterator = iterator_for_value(
            serialized_array,
            FindOptions::ValuesOnly | FindOptions::DeserializeValues | FindOptions::PickField1,
        );

        assert!(iterator.next());
        assert!(
            iterator.value().is_err(),
            "C# Array indexer faults when PickField1 is requested for a one-item array"
        );
    }

    #[test]
    fn value_uses_stack_value_projection_until_vm_return() {
        let source = include_str!("storage_iterator.rs");
        let start = source.find("fn value(&self)").expect("value method exists");
        let end = source[start..]
            .find("fn dispose")
            .map(|offset| start + offset)
            .expect("dispose method follows value");
        let value_method = &source[start..end];

        assert!(value_method.contains("deserialize_stack_value_with_limits"));
        assert!(value_method.contains("StackValue::Array"));
        assert!(value_method.contains("StackValue::Struct"));
        assert!(value_method.contains("stack_value_to_stack_item("));
        assert!(source.contains("StackItem::try_from(value)"));
        assert!(!value_method.contains("BinarySerializer::deserialize("));
        assert!(!value_method.contains("StackItem::from_struct"));
    }
}
