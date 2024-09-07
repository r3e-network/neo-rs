// Copyright (C) 2015-2024 The Neo Project.
//
// memory_store_provider.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::store::Store;

pub struct MemoryStoreProvider;

impl StoreProvider for MemoryStoreProvider {
    fn name(&self) -> &str {
        "MemoryStore"
    }

    fn get_store(&self, _path: &str) -> Box<dyn Store> {
        Box::new(MemoryStore::new())
    }
}
