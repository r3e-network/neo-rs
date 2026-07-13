//! # neo-native-contracts::neo_token
//!
//! Native NEO token governance, voting, and committee behavior.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `constants`: protocol storage prefixes, defaults, reward ratios, and
//!   event names.
//! - `fast_forward`: state-equivalent empty-block reward batching.
//! - `initialize`: genesis storage seeding.
//! - `invoke`: native NEO invocation helpers.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `persist`: block-persist committee and reward hooks.
//! - `providers`: validator and next-consensus read providers.
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.
//! - `transfers`: wallet transfer RPC handlers.
//! - `tests`: Module-local tests and regression coverage.

use neo_config::{Hardfork, ProtocolSettings};
use neo_crypto::ECPoint;
use neo_error::CoreResult;
use neo_execution::{ApplicationEngine, NativeContract, NativeEvent, NativeMethod};
use neo_primitives::UInt160;
use neo_storage::persistence::{DataCache, SeekDirection};
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use neo_vm::StackValue;
use num_bigint::BigInt;

use crate::hashes::NEO_TOKEN_HASH;

mod constants;
mod fast_forward;
mod initialize;
mod invoke;
mod metadata;
mod persist;
mod providers;
mod storage;
mod transfers;

pub(in crate::neo_token) use constants::*;
pub(crate) use constants::{
    NEO_CANDIDATE_STATE_CHANGED_EVENT, NEO_COMMITTEE_CHANGED_EVENT, NEO_VOTE_EVENT,
};
pub(crate) use storage::CachedCommittee;
// Rationale: this storage type is imported for native-contract parity paths
// that are conditionally compiled or reached by integration-only flows.
#[allow(unused_imports)]
use storage::CandidateState;
use storage::NeoAccountStateView;
pub(crate) use storage::candidate_signature_account;

native_contract_handle!(
    /// The NeoToken native contract.
    pub struct NeoToken {
        id: -5,
        contract_name: "NeoToken",
        hash: NEO_TOKEN_HASH,
    }
);

impl NeoToken {
    /// NEP-17 symbol (C# `NeoToken.Symbol => "NEO"`).
    pub const SYMBOL: &'static str = "NEO";
    /// NEP-17 decimals (C# `NeoToken.Decimals => 0`).
    pub const DECIMALS: u8 = 0;
}

impl<P> NativeContract<P> for NeoToken
where
    P: neo_execution::native_contract_provider::NativeContractProvider + 'static,
{
    native_contract_identity!(NeoToken);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::NEO_TOKEN_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    /// C# `NeoToken.OnManifestCompose` (NeoToken.cs:112-122): NEO declares
    /// NEP-27 in addition to NEP-17 once HF_Echidna is enabled at the height.
    fn supported_standards(&self, settings: &ProtocolSettings, block_height: u32) -> Vec<String> {
        if settings.is_hardfork_enabled(Hardfork::HfEchidna, block_height) {
            crate::native_supported_standards(&[crate::NEP17_STANDARD, crate::NEP27_STANDARD])
        } else {
            crate::native_supported_standards(&[crate::NEP17_STANDARD])
        }
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &metadata::NEO_TOKEN_EVENTS
    }

    fn initialize<D, B>(&self, engine: &mut ApplicationEngine<P, D, B>) -> CoreResult<()>
    where
        D: neo_execution::Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        self.initialize_native(engine)
    }

    fn on_persist<D, B>(&self, engine: &mut ApplicationEngine<P, D, B>) -> CoreResult<()>
    where
        D: neo_execution::Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        self.on_persist_native(engine)
    }

    fn post_persist<D, B>(&self, engine: &mut ApplicationEngine<P, D, B>) -> CoreResult<()>
    where
        D: neo_execution::Diagnostic + 'static,
        B: neo_storage::CacheRead,
    {
        self.post_persist_native(engine)
    }

    native_contract_dispatch!(metadata::neo_token_method_bindings);

    /// C# `NEO.GetCommitteeAddress`, exposed through the native-contract seam so
    /// the engine's `check_committee_witness` can verify committee-gated writers
    /// without depending on `neo-native-contracts`.
    fn committee_address<B>(&self, snapshot: &DataCache<B>) -> CoreResult<Option<UInt160>>
    where
        B: neo_storage::CacheRead,
    {
        Ok(Some(self.compute_committee_address(snapshot)?))
    }
}

#[cfg(test)]
#[path = "../tests/neo_token/mod.rs"]
mod tests;
