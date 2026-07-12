//! Historical state-query handlers for StateService RPC.
//!
//! `getstate` and `findstates` both resolve the contract id through the
//! historical MPT root before reading storage entries. The endpoint layer uses
//! the frozen state-provider capability and does not construct snapshots or
//! tries itself.

use neo_execution::contract_state::ContractState;
use neo_primitives::UInt160;
use neo_state_service::{StateProviderError, StateProviderFactory, StateView};
use serde_json::Value;

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;

use super::RpcServerState;
use super::request::{FindStatesRequest, StateKeyRequest};
use super::response::{FindStatesResponse, base64_state_value_to_json};

/// `ContractManagement::ID` (C# `NativeContract.ContractManagement.Id`).
const CONTRACT_MANAGEMENT_ID: i32 = -1;

/// `ContractManagement.Prefix_Contract` — the per-contract record
/// prefix the C# `StatePlugin.GetHistoricalContractState` reads
/// (`const byte prefix = 8`).
const PREFIX_CONTRACT: u8 = 8;

impl RpcServerState {
    /// `getstate(roothash, scripthash, key)` — C# `StatePlugin.GetState`:
    /// resolves the value stored under the historical root and returns
    /// it Base64-encoded.
    ///
    /// C# reads the value through the trie indexer, whose
    /// `KeyNotFoundException` escapes to the generic RPC handler as a
    /// raw .NET `HResult` custom error; this port reports the named
    /// `UnknownStorageItem` (-104) instead — the code the C# plugin
    /// itself uses for the identical condition in `getproof`.
    pub(super) fn get_state(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let factory = Self::state_provider_factory(server)?;
        let request = StateKeyRequest::parse_get_state(params)?;

        let mut state = factory
            .state_by_root(request.root_hash)
            .map_err(|error| Self::state_provider_error("getstate", error))?;
        let contract_id = Self::historical_contract_id(&mut state, &request.script_hash)?;
        let storage_key = Self::storage_key_bytes(contract_id, &request.key);
        let value = state
            .get(&storage_key)
            .map_err(|error| Self::state_provider_error("getstate", error))?
            .ok_or_else(|| RpcException::from(RpcError::unknown_storage_item()))?;
        Ok(base64_state_value_to_json(&value))
    }

    /// `findstates(roothash, scripthash, prefix, [key], [count])` — C#
    /// `StatePlugin.FindStates`: enumerates storage entries under
    /// `prefix`, resuming strictly after the optional `key`, capped at
    /// `count` (default and maximum
    /// [`super::support::MAX_FIND_RESULT_ITEMS`]). The response carries the page,
    /// `truncated`, and Merkle proofs for the first (and, when the
    /// page has more than one entry, last) returned key.
    pub(super) fn find_states(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let factory = Self::state_provider_factory(server)?;
        let request = FindStatesRequest::parse(params)?;

        let mut state = factory
            .state_by_root(request.root_hash)
            .map_err(|error| Self::state_provider_error("findstates", error))?;
        let contract_id = Self::historical_contract_id(&mut state, &request.script_hash)?;

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
        let mut entries = state
            .find(&prefix_key, from_storage_key.as_deref(), request.count + 1)
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
            .map(|(first_key, _)| Self::proof_payload(&mut state, contract_id, first_key))
            .transpose()?;
        let last_proof = if results.len() > 1 {
            results
                .last()
                .map(|(last_key, _)| Self::proof_payload(&mut state, contract_id, last_key))
                .transpose()?
        } else {
            None
        };
        Ok(FindStatesResponse::new(first_proof, last_proof, truncated, results).into_json())
    }

    /// C# `StatePlugin.GetHistoricalContractState`: reads the
    /// `ContractManagement` per-contract record
    /// (`KeyBuilder(-1, 8).Add(scriptHash)`) out of the historical
    /// trie and decodes the interoperable `ContractState` to obtain
    /// the contract id (`UnknownContract` when absent).
    pub(super) fn historical_contract_id<V>(
        state: &mut V,
        script_hash: &UInt160,
    ) -> Result<i32, RpcException>
    where
        V: StateView,
    {
        // Build the raw storage key bytes: id (LE) + prefix + script_hash.
        // Uses StorageKey::create_with_uint160 (ADR-025) instead of hand-rolling
        // the Vec to ensure byte-layout consistency with the rest of the workspace.
        let key = neo_storage::StorageKey::create_with_uint160(
            CONTRACT_MANAGEMENT_ID,
            PREFIX_CONTRACT,
            script_hash,
        )
        .to_array();

        let record = state
            .get(&key)
            .map_err(|error| Self::state_provider_error("contract lookup", error))?
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
    pub(super) fn storage_key_bytes(contract_id: i32, key: &[u8]) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(std::mem::size_of::<i32>() + key.len());
        bytes.extend_from_slice(&contract_id.to_le_bytes());
        bytes.extend_from_slice(key);
        bytes
    }

    /// Maps state-view scan argument failures (`from` not under the
    /// prefix, oversized keys) to `InvalidParams`; anything else is a
    /// resolution failure.
    fn find_error(error: StateProviderError) -> RpcException {
        if let Some(message) = error.invalid_argument_message() {
            RpcException::from(RpcError::invalid_params().with_data(message.to_owned()))
        } else {
            Self::state_provider_error("findstates", error)
        }
    }
}
