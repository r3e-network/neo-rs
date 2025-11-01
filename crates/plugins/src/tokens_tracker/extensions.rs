// Copyright (C) 2015-2025 The Neo Project.
//
// extensions.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo_core::{ByteString, ISerializable, IStore, SeekDirection, StackItem};
use std::collections::HashMap;
use std::num::BigInt;

/// Extension methods for tokens tracker.
/// Matches C# Extensions class exactly
pub struct Extensions;

impl Extensions {
    /// Checks if a stack item is not null.
    /// Matches C# NotNull method
    pub fn not_null(item: &StackItem) -> bool {
        !item.is_null()
    }

    /// Converts a byte span to base64 string.
    /// Matches C# ToBase64 method
    pub fn to_base64(item: &[u8]) -> String {
        if item.is_empty() {
            String::new()
        } else {
            base64::encode(item)
        }
    }

    /// Gets the variable size of a ByteString.
    /// Matches C# GetVarSize method for ByteString
    pub fn get_var_size_byte_string(item: &ByteString) -> usize {
        let length = item.len();
        Self::get_var_size(length) + length
    }

    /// Gets the variable size of a BigInteger.
    /// Matches C# GetVarSize method for BigInteger
    pub fn get_var_size_big_int(item: &BigInt) -> usize {
        let length = item.bits() / 8 + 1; // Approximate byte count
        Self::get_var_size(length) + length
    }

    /// Gets the variable size of a length.
    /// Matches C# GetVarSize method for int
    pub fn get_var_size(length: usize) -> usize {
        if length < 0xFD {
            1
        } else if length <= 0xFFFF {
            3
        } else if length <= 0xFFFFFFFF {
            5
        } else {
            9
        }
    }

    /// Finds items with a prefix.
    /// Matches C# FindPrefix method
    pub fn find_prefix<TKey, TValue>(
        db: &dyn IStore,
        prefix: &[u8],
    ) -> Result<Vec<(TKey, TValue)>, String>
    where
        TKey: ISerializable + Default,
        TValue: ISerializable + Default,
    {
        let mut results = Vec::new();

        for (key, value) in db.find(prefix, SeekDirection::Forward)? {
            if !key.starts_with(prefix) {
                break;
            }

            let key_obj = TKey::deserialize(&mut key[1..].as_ref())?;
            let value_obj = TValue::deserialize(&mut value.as_ref())?;
            results.push((key_obj, value_obj));
        }

        Ok(results)
    }

    /// Finds items in a range.
    /// Matches C# FindRange method
    pub fn find_range<TKey, TValue>(
        db: &dyn IStore,
        start_key: &[u8],
        end_key: &[u8],
    ) -> Result<Vec<(TKey, TValue)>, String>
    where
        TKey: ISerializable + Default,
        TValue: ISerializable + Default,
    {
        let mut results = Vec::new();

        for (key, value) in db.find(start_key, SeekDirection::Forward)? {
            if key.as_slice().cmp(end_key) == std::cmp::Ordering::Greater {
                break;
            }

            let key_obj = TKey::deserialize(&mut key[1..].as_ref())?;
            let value_obj = TValue::deserialize(&mut value.as_ref())?;
            results.push((key_obj, value_obj));
        }

        Ok(results)
    }
}

/// Extension trait for StackItem.
pub trait StackItemExtensions {
    /// Checks if the stack item is not null.
    fn not_null(&self) -> bool;
}

impl StackItemExtensions for StackItem {
    fn not_null(&self) -> bool {
        Extensions::not_null(self)
    }
}

/// Extension trait for byte slices.
pub trait ByteSliceExtensions {
    /// Converts to base64 string.
    fn to_base64(&self) -> String;
}

impl ByteSliceExtensions for [u8] {
    fn to_base64(&self) -> String {
        Extensions::to_base64(self)
    }
}

/// Extension trait for ByteString.
pub trait ByteStringExtensions {
    /// Gets the variable size.
    fn get_var_size(&self) -> usize;
}

impl ByteStringExtensions for ByteString {
    fn get_var_size(&self) -> usize {
        Extensions::get_var_size_byte_string(self)
    }
}

/// Extension trait for BigInt.
pub trait BigIntExtensions {
    /// Gets the variable size.
    fn get_var_size(&self) -> usize;
}

impl BigIntExtensions for BigInt {
    fn get_var_size(&self) -> usize {
        Extensions::get_var_size_big_int(self)
    }
}

/// Extension trait for IStore.
pub trait IStoreExtensions {
    /// Finds items with a prefix.
    fn find_prefix<TKey, TValue>(&self, prefix: &[u8]) -> Result<Vec<(TKey, TValue)>, String>
    where
        TKey: ISerializable + Default,
        TValue: ISerializable + Default;

    /// Finds items in a range.
    fn find_range<TKey, TValue>(
        &self,
        start_key: &[u8],
        end_key: &[u8],
    ) -> Result<Vec<(TKey, TValue)>, String>
    where
        TKey: ISerializable + Default,
        TValue: ISerializable + Default;
}

impl IStoreExtensions for dyn IStore {
    fn find_prefix<TKey, TValue>(&self, prefix: &[u8]) -> Result<Vec<(TKey, TValue)>, String>
    where
        TKey: ISerializable + Default,
        TValue: ISerializable + Default,
    {
        Extensions::find_prefix(self, prefix)
    }

    fn find_range<TKey, TValue>(
        &self,
        start_key: &[u8],
        end_key: &[u8],
    ) -> Result<Vec<(TKey, TValue)>, String>
    where
        TKey: ISerializable + Default,
        TValue: ISerializable + Default,
    {
        Extensions::find_range(self, start_key, end_key)
    }
}
