// Copyright (C) 2015-2025 The Neo Project.
//
// i_committing_handler.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::{
    ledger::{block::Block, blockchain_application_executed::ApplicationExecuted},
    persistence::data_cache::DataCache,
};
use std::any::Any;

/// Committing handler interface matching C# ICommittingHandler exactly
///
/// Note: The `system` parameter uses `dyn Any` to decouple from the concrete
/// NeoSystem type which is now in neo-node. Implementations can downcast
/// to the concrete type if needed.
pub trait ICommittingHandler {
    /// This is the handler of Committing event from Blockchain
    /// Triggered when a new block is committing, and the state is still in the cache.
    /// Matches C# Blockchain_Committing_Handler method
    fn blockchain_committing_handler(
        &self,
        system: &dyn Any,
        block: &Block,
        snapshot: &DataCache,
        application_executed_list: &[ApplicationExecuted],
    );
}
