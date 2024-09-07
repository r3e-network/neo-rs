// Copyright (C) 2015-2024 The Neo Project.
//
// transaction_removed_event_args.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use neo::prelude::Transaction;
use std::collections::HashSet;
use crate::TransactionRemovalReason;

/// Represents the event data of `MemoryPool::transaction_removed`.
pub struct TransactionRemovedEventArgs {
    /// The `Transaction`s that are being removed.
    pub transactions: HashSet<Transaction>,

    /// The reason a transaction was removed.
    pub reason: TransactionRemovalReason,
}

impl TransactionRemovedEventArgs {
    /// Creates a new instance of `TransactionRemovedEventArgs`.
    pub fn new(transactions: HashSet<Transaction>, reason: TransactionRemovalReason) -> Self {
        Self {
            transactions,
            reason,
        }
    }
}
