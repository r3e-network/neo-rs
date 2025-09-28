// Copyright (C) 2015-2025 The Neo Project.
//
// i_write_store.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

/// This interface provides methods to write to the database.
pub trait IWriteStore<TKey, TValue> {
    /// Deletes an entry from the store.
    fn delete(&mut self, key: TKey);

    /// Puts an entry to the store.
    fn put(&mut self, key: TKey, value: TValue);

    /// Puts an entry to the database synchronously.
    fn put_sync(&mut self, key: TKey, value: TValue) {
        self.put(key, value);
    }
}
