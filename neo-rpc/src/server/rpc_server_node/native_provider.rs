//! Native-contract read capabilities for node RPC handlers.
//!
//! Node RPC handlers assemble public node metadata. Dynamic `getversion`
//! protocol fields still come from native Ledger/Policy state, so this module
//! keeps the raw storage reads behind a local provider seam and leaves the
//! handler focused on response assembly.

use neo_config::ProtocolSettings;
use neo_native_contracts::{LedgerContract, PolicyContract};
use neo_primitives::hardfork::Hardfork;
use neo_storage::StorageKey;
use neo_storage::persistence::DataCache;
use num_bigint::BigInt;
use num_traits::ToPrimitive;

use crate::server::ledger_queries;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;

/// C# `LedgerContract.Prefix_CurrentBlock` - current-block pointer.
const LEDGER_PREFIX_CURRENT_BLOCK: u8 = 12;
/// C# `PolicyContract.Prefix_MillisecondsPerBlock` (HF_Echidna).
const POLICY_PREFIX_MILLISECONDS_PER_BLOCK: u8 = 21;
/// C# `PolicyContract.Prefix_MaxValidUntilBlockIncrement` (HF_Echidna).
const POLICY_PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT: u8 = 22;
/// C# `PolicyContract.Prefix_MaxTraceableBlocks` (HF_Echidna).
const POLICY_PREFIX_MAX_TRACEABLE_BLOCKS: u8 = 23;

/// Dynamic protocol fields surfaced by `getversion`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) struct VersionPolicyValues {
    /// Effective block time in milliseconds.
    pub(super) milliseconds_per_block: u32,
    /// Effective traceability window.
    pub(super) max_traceable_blocks: u32,
    /// Effective transaction validity-window increment.
    pub(super) max_valid_until_block_increment: u32,
}

impl VersionPolicyValues {
    /// Static protocol-settings values used before HF_Echidna or when C# would
    /// fall back after a missing native storage key.
    #[must_use]
    pub(super) const fn from_settings(settings: &ProtocolSettings) -> Self {
        Self {
            milliseconds_per_block: settings.milliseconds_per_block,
            max_traceable_blocks: settings.max_traceable_blocks,
            max_valid_until_block_increment: settings.max_valid_until_block_increment,
        }
    }
}

/// Native-contract capabilities required by node RPC handlers.
pub(super) trait NodeNativeProvider {
    /// Returns the C# `NeoSystemExtensions` dynamic Policy values used by
    /// `getversion`.
    fn version_policy_values(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> Result<VersionPolicyValues, RpcException>;
}

/// Factory for node RPC native-contract providers.
pub(super) trait NodeNativeProviderFactory {
    /// Provider returned by this factory.
    type Provider: NodeNativeProvider;

    /// Creates a provider instance.
    fn provider(&self) -> Self::Provider;
}

/// Production provider backed by canonical native-contract storage layouts.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeNodeProvider;

impl NativeNodeProvider {
    /// Creates the production node native provider.
    #[must_use]
    pub(super) const fn new() -> Self {
        Self
    }

    /// C# `NativeContract.Ledger.CurrentIndex(snapshot)` throws when the
    /// pointer key is absent. The Rust ledger provider reports index 0 instead,
    /// so probe key presence first to keep C# fallback semantics exact.
    fn current_index_or_none(&self, snapshot: &DataCache) -> Result<Option<u32>, RpcException> {
        let pointer_key = StorageKey::new(LedgerContract::ID, vec![LEDGER_PREFIX_CURRENT_BLOCK]);
        if snapshot.get(&pointer_key).is_none() {
            return Ok(None);
        }
        ledger_queries::current_index(snapshot)
            .map(Some)
            .map_err(internal_error)
    }

    /// Reads one post-Echidna Policy storage value as C# `(uint)(BigInteger)
    /// snapshot[key]`, with `None` preserving the C# missing-key fallback.
    fn policy_u32_or_none(
        &self,
        snapshot: &DataCache,
        policy_prefix: u8,
    ) -> Result<Option<u32>, RpcException> {
        let key = StorageKey::new(PolicyContract::ID, vec![policy_prefix]);
        match snapshot.get(&key) {
            Some(item) => {
                let value = BigInt::from_signed_bytes_le(&item.value_bytes());
                value.to_u32().map(Some).ok_or_else(|| {
                    internal_error(format!(
                        "Policy storage value under prefix {policy_prefix} is out of u32 range: {value}"
                    ))
                })
            }
            None => Ok(None),
        }
    }
}

impl NodeNativeProvider for NativeNodeProvider {
    fn version_policy_values(
        &self,
        snapshot: &DataCache,
        settings: &ProtocolSettings,
    ) -> Result<VersionPolicyValues, RpcException> {
        let fallback = VersionPolicyValues::from_settings(settings);
        let Some(index) = self.current_index_or_none(snapshot)? else {
            return Ok(fallback);
        };
        if !settings.is_hardfork_enabled(Hardfork::HfEchidna, index) {
            return Ok(fallback);
        }
        Ok(VersionPolicyValues {
            milliseconds_per_block: self
                .policy_u32_or_none(snapshot, POLICY_PREFIX_MILLISECONDS_PER_BLOCK)?
                .unwrap_or(fallback.milliseconds_per_block),
            max_traceable_blocks: self
                .policy_u32_or_none(snapshot, POLICY_PREFIX_MAX_TRACEABLE_BLOCKS)?
                .unwrap_or(fallback.max_traceable_blocks),
            max_valid_until_block_increment: self
                .policy_u32_or_none(snapshot, POLICY_PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT)?
                .unwrap_or(fallback.max_valid_until_block_increment),
        })
    }
}

/// Factory for production node RPC native providers.
#[derive(Clone, Copy, Debug, Default)]
pub(super) struct NativeNodeProviderFactory;

impl NodeNativeProviderFactory for NativeNodeProviderFactory {
    type Provider = NativeNodeProvider;

    fn provider(&self) -> Self::Provider {
        NativeNodeProvider::new()
    }
}
