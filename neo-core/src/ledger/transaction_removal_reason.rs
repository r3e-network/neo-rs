// Copyright (C) 2015-2024 The Neo Project.
//
// transaction_removal_reason.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

/// The reason a transaction was removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionRemovalReason {
    /// The transaction was rejected since it was the lowest priority transaction
    /// and the memory pool capacity was exceeded.
    CapacityExceeded,

    /// The transaction was rejected due to failing re-validation after a block was persisted.
    NoLongerValid,

    /// The transaction was rejected due to conflict with higher priority transactions
    /// with Conflicts attribute.
    Conflict,
}

impl From<TransactionRemovalReason> for u8 {
    fn from(reason: TransactionRemovalReason) -> Self {
        match reason {
            TransactionRemovalReason::CapacityExceeded => 0,
            TransactionRemovalReason::NoLongerValid => 1,
            TransactionRemovalReason::Conflict => 2,
        }
    }
}

impl TryFrom<u8> for TransactionRemovalReason {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(TransactionRemovalReason::CapacityExceeded),
            1 => Ok(TransactionRemovalReason::NoLongerValid),
            2 => Ok(TransactionRemovalReason::Conflict),
            _ => Err(()),
        }
    }
}
