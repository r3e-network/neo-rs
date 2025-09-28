// Copyright (C) 2015-2025 The Neo Project.
//
// helpers.rs mirrors the naming and call sites used by the C# native contracts
// for dBFT consensus wiring, providing a consistent facade for the Rust port.

use crate::cryptography::crypto_utils::ECPoint;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::Contract;
use crate::{UInt160, UInt256};

/// Facade exposing helper methods with the same names/semantics used by the C# port
/// where possible. These route to protocol settings or native contracts when available.
pub struct NativeHelpers;

impl NativeHelpers {
    /// Returns the next block validators from the protocol/native NEO contract.
    /// In the absence of a full native contract, fallback to standby validators
    /// truncated to ValidatorsCount, matching current C# defaults on genesis.
    pub fn get_next_block_validators(settings: &ProtocolSettings) -> Vec<ECPoint> {
        settings.standby_validators()
    }

    /// Computes the next block validators from the protocol/native NEO contract.
    /// Until full native logic is available, reuse the same selection as GetNextBlockValidators.
    pub fn compute_next_block_validators(settings: &ProtocolSettings) -> Vec<ECPoint> {
        settings.standby_validators()
    }

    /// Computes the BFT multi-signature address (C# Contract.GetBFTAddress equivalent).
    pub fn get_bft_address(validators: &[ECPoint]) -> UInt160 {
        let m = validators
            .len()
            .saturating_sub((validators.len().saturating_sub(1)) / 3);
        Contract::create_multi_sig_contract(m, validators).script_hash()
    }

    /// Gets the current block index (height) from the native ledger contract.
    /// Until full integration, returns 0.
    pub fn current_index() -> u32 {
        0
    }

    /// Gets the current block hash from the native ledger contract.
    /// Until full integration, returns zero hash.
    pub fn current_hash() -> UInt256 {
        UInt256::default()
    }

    /// Determines whether the committee should refresh at the given height (+1) according to C# logic.
    /// Until full integration, returns false.
    pub fn should_refresh_committee(_next_height: u32, _committee_members_count: usize) -> bool {
        false
    }
}
