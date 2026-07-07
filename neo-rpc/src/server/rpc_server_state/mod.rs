//! # neo-rpc::server::rpc_server_state
//!
//! State-service RPC endpoint handlers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `proof`: State proof RPC handlers and proof payload codec.
//! - `request`: Typed JSON-RPC request parsing helpers.
//! - `response`: Typed JSON-RPC response construction helpers.
//! - `tests`: Module-local tests and regression coverage.

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::{RpcHandler, RpcServer};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use neo_crypto::mpt_trie::{MptError, Trie};
use neo_execution::contract_state::ContractState;
use neo_primitives::{UInt160, UInt256};
use neo_state_service::StateStore;
use neo_state_service::mpt_store::{MptReadSnapshot, MptStore};
use neo_state_service::state_store::StateStoreLookup;
use serde_json::{Value, json};
use std::sync::Arc;

mod proof;
mod request;
mod response;
use request::{FindStatesRequest, StateKeyRequest, StateRootRequest};
use response::{FindStatesResponse, state_root_to_json};

/// `ContractManagement::ID` (C# `NativeContract.ContractManagement.Id`).
const CONTRACT_MANAGEMENT_ID: i32 = -1;

/// `ContractManagement.Prefix_Contract` — the per-contract record
/// prefix the C# `StatePlugin.GetHistoricalContractState` reads
/// (`const byte prefix = 8`).
const PREFIX_CONTRACT: u8 = 8;

/// C# `StateServiceSettings.MaxFindResultItems` default (the plugin
/// caps every `findstates` page at this many results).
const MAX_FIND_RESULT_ITEMS: usize = 100;

/// RPC handler group for StateService methods.
pub struct RpcServerState;

impl RpcServerState {
    /// Register StateService RPC handlers.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "getstateheight" => Self::get_state_height,
            "getstateroot" => Self::get_state_root,
            "getproof" => Self::get_proof,
            "verifyproof" => Self::verify_proof,
            "getstate" => Self::get_state,
            "findstates" => Self::find_states,
        ]
    }

    fn state_store(server: &RpcServer) -> Result<Arc<StateStore>, RpcException> {
        server.system().state_store().ok_or_else(|| {
            RpcException::from(
                RpcError::internal_server_error().with_data("StateService service not registered"),
            )
        })
    }

    /// Resolves the persisted MPT backend, or reports the same
    /// `UnsupportedState` error the MPT-less build always served.
    fn mpt_store(server: &RpcServer) -> Result<Arc<MptStore>, RpcException> {
        let state_store = Self::state_store(server)?;
        state_store.mpt().ok_or_else(Self::proofs_unsupported)
    }

    fn get_state_height(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let state_store = Self::state_store(server)?;
        // The state-root cache records roots once they are validated, so the
        // local and validated indexes coincide in this build. The verification
        // StateStore is only populated when the (currently dormant) state-root
        // verification pipeline runs; fall back to the live MptStore, which is
        // written by the block-apply pipeline, so a running node reports a real
        // height instead of null.
        let index = state_store
            .current_local_index()
            .or_else(|| {
                Self::mpt_store(server)
                    .ok()
                    .and_then(|mpt| mpt.current_local_root_index())
            })
            .map_or(Value::Null, |index| json!(index));
        Ok(json!({
            "localrootindex": index,
            "validatedrootindex": index}))
    }

    fn get_state_root(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let request = StateRootRequest::parse(params)?;
        let state_store = Self::state_store(server)?;
        let state_root = state_store
            .get_state_root(StateStoreLookup::ByBlockIndex(request.index))
            .or_else(|| {
                // Fall back to the live MptStore (written by apply_block_changes)
                // when the verification StateStore cache is empty.
                Self::mpt_store(server)
                    .ok()
                    .and_then(|mpt| mpt.get_state_root(request.index))
            })
            .ok_or_else(|| RpcException::from(RpcError::unknown_state_root()))?;
        Ok(state_root_to_json(&state_root))
    }

    /// `getstate(roothash, scripthash, key)` — C# `StatePlugin.GetState`:
    /// resolves the value stored under the historical root and returns
    /// it Base64-encoded.
    ///
    /// C# reads the value through the trie indexer, whose
    /// `KeyNotFoundException` escapes to the generic RPC handler as a
    /// raw .NET `HResult` custom error; this port reports the named
    /// `UnknownStorageItem` (-104) instead — the code the C# plugin
    /// itself uses for the identical condition in `getproof`.
    fn get_state(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let mpt = Self::mpt_store(server)?;
        let request = StateKeyRequest::parse_get_state(params)?;

        let snapshot = mpt.snapshot();
        Self::check_root_hash(&snapshot, &request.root_hash)?;
        let mut trie = snapshot.open_trie(Some(request.root_hash));
        let contract_id = Self::historical_contract_id(&mut trie, &request.script_hash)?;
        let storage_key = Self::storage_key_bytes(contract_id, &request.key);
        let value = trie
            .try_get_value(&storage_key)
            .map_err(|err| Self::trie_lookup_error("getstate", &err))?
            .ok_or_else(|| RpcException::from(RpcError::unknown_storage_item()))?;
        Ok(Value::String(BASE64_STANDARD.encode(value)))
    }

    /// `findstates(roothash, scripthash, prefix, [key], [count])` — C#
    /// `StatePlugin.FindStates`: enumerates storage entries under
    /// `prefix`, resuming strictly after the optional `key`, capped at
    /// `count` (default and maximum
    /// [`MAX_FIND_RESULT_ITEMS`]). The response carries the page,
    /// `truncated`, and Merkle proofs for the first (and, when the
    /// page has more than one entry, last) returned key.
    fn find_states(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let mpt = Self::mpt_store(server)?;
        let request = FindStatesRequest::parse(params)?;

        let snapshot = mpt.snapshot();
        Self::check_root_hash(&snapshot, &request.root_hash)?;
        let mut trie = snapshot.open_trie(Some(request.root_hash));
        let contract_id = Self::historical_contract_id(&mut trie, &request.script_hash)?;

        let prefix_key = Self::storage_key_bytes(contract_id, &request.prefix);
        let from_storage_key = request
            .from_key
            .as_ref()
            .filter(|from| !from.is_empty())
            .map(|from| Self::storage_key_bytes(contract_id, from));

        // C# consumes the lazy `Trie.Find` enumerator and breaks once
        // it has seen `count` results plus one probe entry, so the
        // trie traversal never materializes the whole prefix range.
        // `find_limited` is that early break: request exactly
        // `count + 1` entries — the probe's only job is to tell us
        // whether the page is truncated.
        let mut entries = trie
            .find_limited(&prefix_key, from_storage_key.as_deref(), request.count + 1)
            .map_err(Self::find_error)?;
        let truncated = entries.len() > request.count;
        entries.truncate(request.count);
        let results: Vec<(Vec<u8>, Vec<u8>)> = entries
            .into_iter()
            .map(|entry| {
                let key_suffix = entry.key.get(std::mem::size_of::<i32>()..).unwrap_or(&[]);
                (key_suffix.to_vec(), entry.value)
            })
            .collect();

        let first_proof = results
            .first()
            .map(|(first_key, _)| Self::proof_payload(&mut trie, contract_id, first_key))
            .transpose()?;
        let last_proof = if results.len() > 1 {
            results
                .last()
                .map(|(last_key, _)| Self::proof_payload(&mut trie, contract_id, last_key))
                .transpose()?
        } else {
            None
        };
        Ok(FindStatesResponse::new(first_proof, last_proof, truncated, results).into_json())
    }

    /// C# `StatePlugin.CheckRootHash`: without `FullState`, only the
    /// current local root may be queried (`UnsupportedState`
    /// otherwise, with the same diagnostic data string — C#
    /// interpolates `bool.ToString()`, so the flag reads
    /// `True`/`False`).
    ///
    /// The check runs against the request's own store snapshot (C#
    /// reads the live `CurrentLocalRootHash` just before opening the
    /// snapshot; gating on the snapshot's value closes that race
    /// window without changing the accepted set).
    fn check_root_hash(
        snapshot: &MptReadSnapshot,
        root_hash: &UInt256,
    ) -> Result<(), RpcException> {
        let full_state = snapshot.full_state();
        let current = snapshot.current_local_root_hash();
        if !full_state && current.as_ref() != Some(root_hash) {
            let full_state_text = if full_state { "True" } else { "False" };
            let current_text = current.map(|hash| hash.to_string()).unwrap_or_default();
            return Err(RpcException::from(RpcError::unsupported_state().with_data(
                format!("fullState:{full_state_text},current:{current_text},rootHash:{root_hash}"),
            )));
        }
        Ok(())
    }

    /// C# `StatePlugin.GetHistoricalContractState`: reads the
    /// `ContractManagement` per-contract record
    /// (`KeyBuilder(-1, 8).Add(scriptHash)`) out of the historical
    /// trie and decodes the interoperable `ContractState` to obtain
    /// the contract id (`UnknownContract` when absent).
    fn historical_contract_id(
        trie: &mut Trie<MptReadSnapshot>,
        script_hash: &UInt160,
    ) -> Result<i32, RpcException> {
        // Build the raw storage key bytes: id (LE) + prefix + script_hash.
        // Uses StorageKey::create_with_uint160 (ADR-025) instead of hand-rolling
        // the Vec to ensure byte-layout consistency with the rest of the workspace.
        let key = neo_storage::StorageKey::create_with_uint160(
            CONTRACT_MANAGEMENT_ID,
            PREFIX_CONTRACT,
            script_hash,
        )
        .to_array();

        let record = trie
            .try_get_value(&key)
            .map_err(|err| Self::trie_lookup_error("contract lookup", &err))?
            .ok_or_else(|| RpcException::from(RpcError::unknown_contract()))?;
        let contract = ContractState::deserialize_contract_record(&record).map_err(|err| {
            RpcException::from(
                RpcError::internal_server_error()
                    .with_data(format!("malformed contract record in state trie: {err}")),
            )
        })?;
        Ok(contract.id)
    }

    /// Serializes `(contract_id, key)` as the C# `StorageKey.ToArray()`
    /// bytes the state trie is keyed by: little-endian `i32` id + key.
    fn storage_key_bytes(contract_id: i32, key: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(std::mem::size_of::<i32>() + key.len());
        bytes.extend_from_slice(&contract_id.to_le_bytes());
        bytes.extend_from_slice(key);
        bytes
    }

    /// Maps a trie resolution failure (e.g. a root hash this store has
    /// never persisted) to an internal error.
    ///
    /// C# surfaces the same condition as an uncaught
    /// `InvalidOperationException` converted to a raw `HResult` custom
    /// error; a named internal error with the MPT message is this
    /// port's equivalent.
    fn trie_lookup_error(context: &str, err: &MptError) -> RpcException {
        RpcException::from(
            RpcError::internal_server_error()
                .with_data(format!("{context}: MPT resolution failed: {err}")),
        )
    }

    /// Maps `Trie::find` argument failures (`from` not under the
    /// prefix, oversized keys) to `InvalidParams`; anything else is a
    /// resolution failure.
    fn find_error(err: MptError) -> RpcException {
        match err {
            MptError::InvalidOperation(message) | MptError::Key(message) => {
                RpcException::from(RpcError::invalid_params().with_data(message))
            }
            other => Self::trie_lookup_error("findstates", &other),
        }
    }

    /// The state-root cache does not persist the MPT trie, so queries
    /// that must walk historical tries cannot be answered.
    fn proofs_unsupported() -> RpcException {
        RpcException::from(RpcError::unsupported_state().with_data(
            "the state service in this build records validated state roots only and does not \
             persist the MPT trie required for state/proof queries",
        ))
    }
}

#[cfg(test)]
#[path = "../../tests/server/handlers/rpc_server_state.rs"]
mod tests;
