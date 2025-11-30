// Copyright (C) 2015-2025 The Neo Project.
//
// contains_transaction_type.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

/// Represents the type of transaction containment.
/// Matches C# ContainsTransactionType enum exactly
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ContainsTransactionType {
    /// Transaction does not exist.
    /// Matches C# NotExist variant
    NotExist,

    /// Transaction exists in the memory pool.
    /// Matches C# ExistsInPool variant
    ExistsInPool,

    /// Transaction exists in the ledger.
    /// Matches C# ExistsInLedger variant
    ExistsInLedger,
}
