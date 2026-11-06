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
use neo_primitives::{BigDecimal, UInt160};

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

#[cfg(test)]
mod tests {
    use super::*;

    fn output_with_amount(value: neo_primitives::BigDecimal) -> TransferOutput {
        TransferOutput {
            asset_id: UInt160::zero(),
            value,
            script_hash: UInt160::zero(),
            data: None,
        }
    }

    #[test]
    fn amount_accepts_authoritative_big_decimal_without_precision_loss() {
        let amount = neo_primitives::BigDecimal::parse(
            "123456789012345678901234567890.123456789012345678",
            18,
        )
        .expect("arbitrary-precision transfer amount");
        let output = output_with_amount(amount);
        let value: neo_primitives::BigDecimal = output.value;

        assert_eq!(value.decimals(), 18);
        assert_eq!(
            value.value().to_string(),
            "123456789012345678901234567890123456789012345678"
        );
        assert_eq!(value.to_string(), "12345678901234567890.123456789012345678");
    }

    #[test]
    fn amount_rescaling_preserves_precision_and_rejects_rounding() {
        let output = output_with_amount(
            neo_primitives::BigDecimal::parse("100.00000001", 8).expect("NEP-17 amount"),
        );
        let scaled = output.value.change_decimals(18).expect("exact rescaling");

        assert_eq!(scaled.value().to_string(), "100000000010000000000");
        assert_eq!(scaled.decimals(), 18);
        assert_eq!(scaled.change_decimals(8).unwrap(), output.value);
        assert!(output.value.change_decimals(7).is_err());
        assert_eq!(output.value.value().to_string(), "10000000001");
        assert_eq!(output.value.decimals(), 8);
    }
}
