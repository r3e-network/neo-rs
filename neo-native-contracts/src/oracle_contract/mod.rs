//! # neo-native-contracts::oracle_contract
//!
//! Native Oracle contract request, response, and fee behavior.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `constants`: request limits, storage prefixes, default pricing, and event
//!   names.
//! - `initialize`: genesis request counter and price seeding.
//! - `invoke`: native method dispatch for request/finish/verify.
//! - `metadata`: Native contract metadata and descriptor helpers.
//! - `persist`: post-persist response cleanup and oracle-node reward minting.
//! - `request`: oracle request records and lifecycle helpers.
//! - `storage`: Storage contexts, key builders, and storage item helpers for
//!   execution.
//! - `tests`: Module-local tests and regression coverage.

use crate::hashes::ORACLE_CONTRACT_HASH;
use neo_config::{Hardfork, ProtocolSettings};
use neo_error::CoreResult;
use neo_execution::native_contract::OracleRequestDetails;
use neo_execution::{ApplicationEngine, NativeContract, NativeEvent, NativeMethod};
use neo_storage::persistence::DataCache;

mod constants;
mod initialize;
mod invoke;
mod metadata;
mod persist;
mod request;
mod storage;

pub(in crate::oracle_contract) use constants::*;
pub(crate) use constants::{ORACLE_REQUEST_EVENT, ORACLE_RESPONSE_EVENT};
pub use request::OracleRequest;

native_contract_handle!(
    /// Static accessor for the OracleContract native contract.
    pub struct OracleContract {
        id: -9,
        contract_name: "OracleContract",
        hash: ORACLE_CONTRACT_HASH,
    }
);

impl NativeContract for OracleContract {
    native_contract_identity!(OracleContract);

    fn methods(&self) -> &[NativeMethod] {
        &metadata::ORACLE_CONTRACT_METHODS
    }

    fn supports_empty_block_fast_forward(&self) -> bool {
        true
    }

    fn event_descriptors(&self) -> &[NativeEvent] {
        &metadata::ORACLE_CONTRACT_EVENTS
    }

    /// C# `OracleContract.Activations => [null, HF_Faun]` (OracleContract.cs:56):
    /// active from genesis, but the manifest's supported standards update at
    /// Faun, so the Faun boundary must refresh the stored contract state.
    fn activations(&self) -> &'static [Hardfork] {
        &[Hardfork::HfFaun]
    }

    /// C# `OracleContract.OnManifestCompose` (OracleContract.cs:58-64): NEP-30
    /// once HF_Faun is enabled at the height; no standards before it.
    fn supported_standards(&self, settings: &ProtocolSettings, block_height: u32) -> Vec<String> {
        if settings.is_hardfork_enabled(Hardfork::HfFaun, block_height) {
            crate::native_supported_standards(&[crate::NEP30_STANDARD])
        } else {
            Vec::new()
        }
    }

    /// Url + original txid pair consumed by the engine's oracle-response
    /// witness path (`CheckWitness` signer inheritance).
    ///
    /// C# `GetRequest(...)` exposed through the native-contract seam so the
    /// engine can resolve oracle-response witnesses without depending on
    /// `neo-native-contracts`.
    fn oracle_request_url_full(
        &self,
        snapshot: &DataCache,
        id: u64,
    ) -> CoreResult<Option<OracleRequestDetails>> {
        Ok(self
            .read_request(snapshot, id)?
            .map(|request| OracleRequestDetails::new(request.url, request.original_tx_id)))
    }

    fn initialize(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        self.initialize_native(engine)
    }

    fn post_persist(&self, engine: &mut ApplicationEngine) -> CoreResult<()> {
        self.post_persist_native(engine)
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
        self.invoke_native(engine, method, args)
    }

    native_contract_resolved_invoke!(metadata::ORACLE_CONTRACT_METHOD_BINDINGS);
}

#[cfg(test)]
#[path = "../tests/oracle_contract/mod.rs"]
mod tests;
