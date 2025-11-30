// Copyright (C) 2015-2025 The Neo Project.
//
// i_transaction_added_handler.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.
use crate::network::p2p::payloads::Transaction;

/// Transaction added handler interface matching C# ITransactionAddedHandler exactly
pub trait ITransactionAddedHandler {
    /// The handler of TransactionAdded event from the MemoryPool.
    /// Triggered when a transaction is added to the MemoryPool.
    /// Matches C# MemoryPool_TransactionAdded_Handler method
    fn memory_pool_transaction_added_handler(&self, sender: &dyn std::any::Any, tx: &Transaction);
}
