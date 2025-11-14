// Copyright (C) 2015-2025 The Neo Project.
//
// helpers.rs mirrors the naming and call sites used by the C# native contracts
// for dBFT consensus wiring, providing a consistent facade for the Rust port.

use crate::cryptography::crypto_utils::ECPoint;
use crate::neo_system::NeoSystemContext;
use crate::persistence::DataCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::NeoToken;
use crate::smart_contract::Contract;
use crate::{UInt160, UInt256};
use once_cell::sync::Lazy;
use std::sync::{Arc, RwLock, Weak};

static SYSTEM_CONTEXT: Lazy<RwLock<Option<Weak<NeoSystemContext>>>> =
    Lazy::new(|| RwLock::new(None));

/// Facade exposing helper methods with the same names/semantics used by the C# port
/// where possible. These route to protocol settings or native contracts when available.
pub struct NativeHelpers;

impl NativeHelpers {
    /// Attaches the running `NeoSystemContext` so helper methods can source live
    /// blockchain data when available.
    pub fn attach_system_context(context: Arc<NeoSystemContext>) {
        let mut guard = SYSTEM_CONTEXT
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = Some(Arc::downgrade(&context));
    }

    #[cfg(test)]
    pub fn clear_system_context() {
        let mut guard = SYSTEM_CONTEXT
            .write()
            .unwrap_or_else(|poisoned| poisoned.into_inner());
        *guard = None;
    }

    pub fn context() -> Option<Arc<NeoSystemContext>> {
        let weak = SYSTEM_CONTEXT
            .read()
            .ok()
            .and_then(|guard| guard.as_ref().cloned());
        weak.and_then(|ctx| ctx.upgrade())
    }

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

    /// Computes the multi-signature committee address from the current committee list.
    /// Until the native NEO contract is fully ported, this uses the standby committee from
    /// protocol settings to mirror the default initialization path in the C# implementation.
    pub fn committee_address(settings: &ProtocolSettings, snapshot: Option<&DataCache>) -> UInt160 {
        let committee = snapshot
            .and_then(|cache| NeoToken::new().committee_from_snapshot(cache))
            .filter(|members| !members.is_empty())
            .unwrap_or_else(|| settings.standby_committee.clone());

        if committee.is_empty() {
            return UInt160::default();
        }

        let len = committee.len();
        let m = len.saturating_sub((len.saturating_sub(1)) / 2);
        Contract::create_multi_sig_contract(m, &committee).script_hash()
    }

    /// Gets the current block index (height) from the native ledger contract.
    /// Falls back to 0 when no runtime context has been attached yet.
    pub fn current_index() -> u32 {
        Self::context()
            .map(|ctx| ctx.current_block_index())
            .unwrap_or(0)
    }

    /// Gets the current block hash from the native ledger contract.
    /// Until the runtime context is available this returns zero hash.
    pub fn current_hash() -> UInt256 {
        if let Some(context) = Self::context() {
            let height = context.current_block_index();
            if let Some(hash) = context.block_hash_at(height) {
                return hash;
            }
        }
        UInt256::default()
    }

    /// Determines whether the committee should refresh at the given height (+1) according to C# logic.
    pub fn should_refresh_committee(next_height: u32, committee_members_count: usize) -> bool {
        if committee_members_count == 0 {
            return false;
        }
        next_height % (committee_members_count as u32) == 0
    }
}
