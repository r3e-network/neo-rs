// Copyright (C) 2015-2025 The Neo Project.
//
// i_store_provider.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::i_store::IStore;
use std::sync::Arc;

/// A provider used to create IStore instances.
pub trait IStoreProvider: Send + Sync {
    /// Gets the name of the IStoreProvider.
    fn name(&self) -> &str;

    /// Creates a new instance of the IStore interface.
    fn get_store(&self, path: &str) -> Arc<dyn IStore>;
}
