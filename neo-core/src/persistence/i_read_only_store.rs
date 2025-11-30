// Copyright (C) 2015-2025 The Neo Project.
//
// i_read_only_store.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::seek_direction::SeekDirection;
use crate::smart_contract::{StorageItem, StorageKey};

/// This interface provides methods to read from the database.
pub trait IReadOnlyStore: IReadOnlyStoreGeneric<StorageKey, StorageItem> {}

/// This interface provides methods to read from the database (generic version).
pub trait IReadOnlyStoreGeneric<TKey, TValue>
where
    TKey: Clone,
    TValue: Clone,
{
    /// Reads a specified entry from the database.
    /// Returns the data of the entry, or None if it doesn't exist.
    fn try_get(&self, key: &TKey) -> Option<TValue>;

    /// Determines whether the database contains the specified entry.
    fn contains(&self, key: &TKey) -> bool {
        self.try_get(key).is_some()
    }

    /// Finds the entries starting with the specified prefix.
    fn find(
        &self,
        key_prefix: Option<&TKey>,
        direction: SeekDirection,
    ) -> Box<dyn Iterator<Item = (TKey, TValue)> + '_>;

    /// Gets the entry with the specified key.
    /// Panics if the key is not found.
    fn get(&self, key: &TKey) -> TValue {
        self.try_get(key).expect("Key not found")
    }
}
