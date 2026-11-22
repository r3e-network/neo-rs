use super::*;

// Implement PartialEq to allow stack items to be compared and used as keys in collections
impl PartialEq for StackItem {
    fn eq(&self, other: &Self) -> bool {
        self.equals(other).unwrap_or(false)
    }
}

impl Eq for StackItem {}
// Implement PartialOrd and Ord to allow stack items to be used as keys in BTreeMap
// Production-ready implementation matching C# StackItem comparison exactly
impl PartialOrd for StackItem {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for StackItem {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        // Production-ready ordering based on C# stack_item comparison rules
        // 1. First compare by type (matches C# stack_item type hierarchy)
        let type_order = self
            .stack_item_type()
            .to_byte()
            .cmp(&other.stack_item_type().to_byte());
        if type_order != std::cmp::Ordering::Equal {
            return type_order;
        }

        // 2. Compare values within the same type
        match (self, other) {
            (Self::Null, Self::Null) => std::cmp::Ordering::Equal,
            (Self::Boolean(a), Self::Boolean(b)) => a.cmp(b),
            (Self::Integer(a), Self::Integer(b)) => a.cmp(b),
            (Self::ByteString(a), Self::ByteString(b)) => a.cmp(b),
            (Self::Buffer(a), Self::Buffer(b)) => a.cmp(b),
            (Self::ByteString(a), Self::Buffer(b)) => b.with_data(|data| a.as_slice().cmp(data)),
            (Self::Buffer(a), Self::ByteString(b)) => a.with_data(|data| data.cmp(b.as_slice())),
            (Self::Pointer(a), Self::Pointer(b)) => a.cmp(b),
            (Self::Array(a), Self::Array(b)) => cmp_stack_item_sequences(a.iter(), b.iter()),
            (Self::Struct(a), Self::Struct(b)) => cmp_stack_item_sequences(a.iter(), b.iter()),
            (Self::Map(a), Self::Map(b)) => {
                // Compare maps by size first, then by sorted key-value pairs
                let len_cmp = a.len().cmp(&b.len());
                if len_cmp != std::cmp::Ordering::Equal {
                    return len_cmp;
                }

                let a_items = a.items();
                let b_items = b.items();
                let mut a_pairs: Vec<_> = a_items.iter().collect();
                let mut b_pairs: Vec<_> = b_items.iter().collect();
                a_pairs.sort_by(|x, y| x.0.cmp(y.0));
                b_pairs.sort_by(|x, y| x.0.cmp(y.0));

                for ((key_a, val_a), (key_b, val_b)) in a_pairs.iter().zip(b_pairs.iter()) {
                    let key_cmp = key_a.cmp(key_b);
                    if key_cmp != std::cmp::Ordering::Equal {
                        return key_cmp;
                    }
                    let val_cmp = val_a.cmp(val_b);
                    if val_cmp != std::cmp::Ordering::Equal {
                        return val_cmp;
                    }
                }
                std::cmp::Ordering::Equal
            }
            _ => {
                let _self_discriminant = std::mem::discriminant(self);
                let _other_discriminant = std::mem::discriminant(other);
                // based on the variant order in the enum
                match (self, other) {
                    (Self::Null, _) => std::cmp::Ordering::Less,
                    (_, Self::Null) => std::cmp::Ordering::Greater,
                    (Self::Boolean(_), Self::Integer(_)) => std::cmp::Ordering::Less,
                    (Self::Integer(_), Self::Boolean(_)) => std::cmp::Ordering::Greater,
                    (Self::Boolean(_), Self::ByteString(_)) => std::cmp::Ordering::Less,
                    (Self::ByteString(_), Self::Boolean(_)) => std::cmp::Ordering::Greater,
                    (Self::Integer(_), Self::ByteString(_)) => std::cmp::Ordering::Less,
                    (Self::ByteString(_), Self::Integer(_)) => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal, // Same types that we haven't handled above
                }
            }
        }
    }
}

fn cmp_stack_item_sequences(
    left: impl ExactSizeIterator<Item = StackItem>,
    right: impl ExactSizeIterator<Item = StackItem>,
) -> std::cmp::Ordering {
    let len_cmp = left.len().cmp(&right.len());
    if len_cmp != std::cmp::Ordering::Equal {
        return len_cmp;
    }

    for (item_a, item_b) in left.zip(right) {
        let item_cmp = item_a.cmp(&item_b);
        if item_cmp != std::cmp::Ordering::Equal {
            return item_cmp;
        }
    }
    std::cmp::Ordering::Equal
}

pub(super) const fn combine_hash(current: i32, value: i32) -> i32 {
    current.wrapping_mul(397).wrapping_add(value)
}

pub(super) fn hash_bytes(bytes: &[u8]) -> i32 {
    bytes
        .iter()
        .fold(17, |hash, byte| combine_hash(hash, i32::from(*byte)))
}
