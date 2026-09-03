// Copyright (C) 2015-2025 The Neo Project.
//
// transfer_output.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.
use crate::big_decimal::BigDecimal;
use neo_primitives::UInt160;

/// Represents an output of a transfer.
/// Matches C# TransferOutput class exactly
pub struct TransferOutput {
    /// The id of the asset to transfer.
    /// Matches C# AssetId field
    pub asset_id: UInt160,

    /// The amount of the asset to transfer.
    /// Matches C# Value field
    pub value: BigDecimal,

    /// The account to transfer to.
    /// Matches C# ScriptHash field
    pub script_hash: UInt160,

    /// The object to be passed to the transfer method of NEP-17.
    /// Matches C# Data field
    pub data: Option<Box<dyn std::any::Any>>,
}
