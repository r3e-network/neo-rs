// Copyright (C) 2015-2025 The Neo Project.
//
// i_committed_handler.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.
use crate::{ledger::block::Block, neo_system::NeoSystem};

/// Committed handler interface matching C# ICommittedHandler exactly
pub trait ICommittedHandler {
    /// This is the handler of Committed event from Blockchain
    /// Triggered after a new block is Committed, and state has being updated.
    /// Matches C# Blockchain_Committed_Handler method
    fn blockchain_committed_handler(&self, system: &NeoSystem, block: &Block);
}
