// Copyright (C) 2015-2025 The Neo Project.
//
// i_transaction_removed_handler.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.
use crate::ledger::transaction_removed_event_args::TransactionRemovedEventArgs;

/// Transaction removed handler interface matching C# ITransactionRemovedHandler exactly
pub trait ITransactionRemovedHandler {
    /// Handler of TransactionRemoved event from MemoryPool
    /// Triggered when a transaction is removed to the MemoryPool.
    /// Matches C# MemoryPool_TransactionRemoved_Handler method
    fn memory_pool_transaction_removed_handler(
        &self,
        sender: &dyn std::any::Any,
        tx: &TransactionRemovedEventArgs,
    );
}
