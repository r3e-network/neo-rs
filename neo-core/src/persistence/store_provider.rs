// Copyright (C) 2015-2025 The Neo Project.
//
// store_provider.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::store::Store;
use crate::error::CoreResult;
use std::any::Any;
use std::sync::Arc;

/// A provider used to create Store instances.
pub trait StoreProvider: Send + Sync + Any {
    /// Gets the name of the StoreProvider.
    fn name(&self) -> &str;

    /// Creates a new instance of the Store interface.
    fn get_store(&self, path: &str) -> CoreResult<Arc<dyn Store>>;
}
