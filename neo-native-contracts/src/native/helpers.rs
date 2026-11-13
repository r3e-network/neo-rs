// Copyright (C) 2015-2025 The Neo Project.
//
// helpers.rs mirrors the naming and call sites used by the C# native contracts
// for dBFT consensus wiring, providing a consistent facade for the Rust port.

use super::NeoToken;
use neo_crypto::ECPoint;
use neo_primitives::{UInt160, UInt256};
use neo_protocol::ProtocolSettings;
use neo_smart_contract::contract::Contract;
use neo_smart_contract::error::{CoreError, CoreResult};
use neo_storage::persistence::DataCache;
use neo_vm::BinarySerializer;
use neo_vm::StackValue;

pub use neo_smart_contract::application_engine::NativeChainQueries;

/// Parses a `UInt160` from a raw byte argument, returning a descriptive
/// native-contract error when the bytes are invalid.
pub(crate) fn parse_uint160_arg(arg: &[u8], label: &str) -> CoreResult<UInt160> {
    UInt160::from_bytes(arg).map_err(|_| CoreError::native_contract(format!("Invalid {label}")))
}

/// Serializes a [`StackValue`] to bytes using default engine limits,
/// mapping errors to [`CoreError::native_contract`].
///
/// This consolidates the repeated pattern:
/// ```ignore
/// BinarySerializer::serialize_stack_value(&val, &ExecutionEngineLimits::default())
///     .map_err(CoreError::native_contract)?
/// ```
pub(crate) fn serialize_stack_value_native(value: &StackValue) -> CoreResult<Vec<u8>> {
    use neo_vm::ExecutionEngineLimits;
    BinarySerializer::serialize_stack_value(value, &ExecutionEngineLimits::default())
        .map_err(CoreError::native_contract)
}

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

    /// Gets the supplied composition's current height, or zero without a provider.
    pub fn current_index(queries: Option<&dyn NativeChainQueries>) -> u32 {
        queries.map_or(
            0,
            neo_smart_contract::application_engine::NativeChainQueries::current_block_index,
        )
    }

    /// Gets the hash at the supplied composition's current height, or zero if unavailable.
    pub fn current_hash(queries: Option<&dyn NativeChainQueries>) -> UInt256 {
        if let Some(queries) = queries {
            let height = queries.current_block_index();
            if let Some(hash) = queries.block_hash_at(height) {
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
        let count_u32 = u32::try_from(committee_members_count).unwrap_or(u32::MAX);
        next_height.is_multiple_of(count_u32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    struct Queries {
        height: u32,
        hash: Option<UInt256>,
        calls: Mutex<Vec<Option<u32>>>,
    }

    impl NativeChainQueries for Queries {
        fn current_block_index(&self) -> u32 {
            self.calls.lock().unwrap().push(None);
            self.height
        }

        fn block_hash_at(&self, index: u32) -> Option<UInt256> {
            self.calls.lock().unwrap().push(Some(index));
            self.hash
        }
    }

    #[test]
    fn provider_free_helpers_keep_zero_fallbacks() {
        assert_eq!(NativeHelpers::current_index(None), 0);
        assert_eq!(NativeHelpers::current_hash(None), UInt256::zero());
    }

    #[test]
    fn current_hash_reads_height_before_hash_and_keeps_missing_hash_fallback() {
        let queries = Queries {
            height: 42,
            hash: None,
            calls: Mutex::new(Vec::new()),
        };
        assert_eq!(NativeHelpers::current_hash(Some(&queries)), UInt256::zero());
        assert_eq!(*queries.calls.lock().unwrap(), vec![None, Some(42)]);
    }

    #[test]
    fn explicit_query_providers_do_not_share_state() {
        let first = Queries {
            height: 3,
            hash: Some(UInt256::from_bytes(&[3; 32]).unwrap()),
            calls: Mutex::new(Vec::new()),
        };
        let second = Queries {
            height: 9,
            hash: Some(UInt256::from_bytes(&[9; 32]).unwrap()),
            calls: Mutex::new(Vec::new()),
        };
        assert_eq!(NativeHelpers::current_index(Some(&first)), 3);
        assert!(second.calls.lock().unwrap().is_empty());
        assert_eq!(
            NativeHelpers::current_hash(Some(&second)),
            second.hash.unwrap()
        );
        assert_eq!(*first.calls.lock().unwrap(), vec![None]);
    }
}
