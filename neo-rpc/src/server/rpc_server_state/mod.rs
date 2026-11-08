//! State Service RPC endpoints
//!
//! This module implements RPC handlers for Neo's State Service.
//! Reference: C# Neo.Plugins.StateService RpcServer methods
//!
//! Methods:
//! - `getstateroot` - Get state root by index
//! - `getstateheight` - Get current state service height
//! - `getstate` - Get state value with proof
//! - `findstates` - Find multiple state values
//! - `getproof` - Get Merkle proof for a storage key
//! - `verifyproof` - Verify a state proof

use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_io::Serializable;
use neo_native_contracts::native::contract_management::ContractManagement;
use neo_primitives::UInt256;
use neo_state_service::state_store::{StateStore, STATE_STORE_SERVICE};
use neo_storage::StorageKey;
use neo_system::state_service::StateRoot;
use serde_json::{Map, Value, json};

use super::rpc_error::RpcError;
use super::rpc_exception::RpcException;
use super::rpc_helpers::internal_error;
use super::rpc_server::{RpcHandler, RpcServer};

/// State Service RPC implementation.
pub struct RpcServerState;

impl RpcServerState {
    /// Registers all State Service RPC handlers.
    ///
    /// Available methods:
    /// - `getstateroot` - Get state root by block index
    /// - `getstateheight` - Get local and validated state root heights
    /// - `getproof` - Get Merkle proof for storage key
    /// - `verifyproof` - Verify a state proof
    /// - `getstate` - Get state value from trie
    /// - `findstates` - Find multiple state values by prefix
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "getstateroot" => Self::get_state_root,
            "getstateheight" => Self::get_state_height,
            "getproof" => Self::get_proof,
            "verifyproof" => Self::verify_proof,
            "getstate" => Self::get_state,
            "findstates" => Self::find_states,
        ]
    }

    /// `getstateroot` - Get state root by block index.
    ///
    /// Parameters:
    /// - `index` (number): Block index
    ///
    /// Returns:
    /// - State root object with version, index, roothash, and witnesses
    ///
    /// Reference: C# Neo.Plugins.StateService RpcServer.GetStateRoot
    pub fn get_state_root(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let state_store = Self::get_state_store(server)?;

        let index = params
            .first()
            .and_then(|v| v.as_u64())
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| {
                RpcException::from(
                    RpcError::invalid_params().with_data("index parameter required (block height)"),
                )
            })?;

        let state_root = state_store
            .get_state_root(index)
            .ok_or_else(|| RpcException::from(RpcError::unknown_state_root()))?;

        Ok(Self::state_root_to_json(&state_root))
    }

    /// `getstateheight` - Get current state service height.
    ///
    /// Returns:
    /// - Object with localrootindex and validatedrootindex
    ///
    /// Reference: C# Neo.Plugins.StateService RpcServer.GetStateHeight
    pub fn get_state_height(
        server: &RpcServer,
        _params: &[Value],
    ) -> Result<Value, RpcException> {
        let state_store = Self::get_state_store(server)?;

        let local_root_index = state_store
            .current_local_index()
            .unwrap_or(0);

        let validated_root_index = state_store
            .current_validated_index()
            .unwrap_or(0);

        Ok(json!({
            "localrootindex": local_root_index,
            "validatedrootindex": validated_root_index,
        }))
    }

    /// `getproof` - Get Merkle proof for a storage key.
    ///
    /// Parameters:
    /// - `roothash` (string): State root hash (UInt256)
    /// - `scripthash` (string): Contract script hash (UInt160)
    /// - `key` (string): Storage key (base64-encoded)
    ///
    /// Returns:
    /// - Base64-encoded proof string
    ///
    /// Reference: C# Neo.Plugins.StateService RpcServer.GetProof
    pub fn get_proof(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let state_store = Self::get_state_store(server)?;

        let root_hash = Self::parse_uint256(params, 0, "roothash")?;
        let script_hash = Self::parse_uint160(params, 1, "scripthash")?;
        let key_base64 = Self::parse_string(params, 2, "key")?;

        let key_bytes = BASE64_STANDARD.decode(key_base64).map_err(|err| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("Invalid base64 key: {}", err)),
            )
        })?;

        // Resolve contract ID from script hash
        let store = server.system().store_cache();
        let contract_mgmt = ContractManagement::new();
        let contract = contract_mgmt
            .get_contract_by_hash(&store, &script_hash)
            .map_err(internal_error)?
            .ok_or_else(|| {
                RpcException::from(RpcError::invalid_params().with_data("Contract not found"))
            })?;

        let storage_key = StorageKey::new(contract.id, key_bytes);

        // Get proof from state store
        let snapshot = state_store.get_snapshot_by_root(root_hash).ok_or_else(|| {
            if !state_store.full_state_enabled() {
                RpcException::from(RpcError::unsupported_state())
            } else {
                RpcException::from(RpcError::unknown_state_root())
            }
        })?;

        let proof = snapshot
            .trie
            .try_get_proof(&storage_key.as_bytes())
            .map_err(internal_error)?
            .unwrap_or_default();

        let proof_nodes: Vec<Vec<u8>> = proof.into_iter().collect();
        let encoded = StateStore::encode_proof_payload(&storage_key.as_bytes(), &proof_nodes);

        Ok(Value::String(BASE64_STANDARD.encode(&encoded)))
    }

    /// `verifyproof` - Verify a state proof.
    ///
    /// Parameters:
    /// - `roothash` (string): State root hash (UInt256)
    /// - `proof` (string): Base64-encoded proof
    ///
    /// Returns:
    /// - Base64-encoded value if proof is valid, empty string if key doesn't exist
    ///
    /// Reference: C# Neo.Plugins.StateService RpcServer.VerifyProof
    pub fn verify_proof(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let state_store = Self::get_state_store(server)?;

        let root_hash = Self::parse_uint256(params, 0, "roothash")?;
        let proof_base64 = Self::parse_string(params, 1, "proof")?;

        let proof_bytes = BASE64_STANDARD.decode(proof_base64).map_err(|err| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("Invalid base64 proof: {}", err)),
            )
        })?;

        let (key, proof_nodes) =
            StateStore::decode_proof_payload(&proof_bytes).map_err(|err| {
                RpcException::from(
                    RpcError::invalid_params()
                        .with_data(format!("Invalid proof payload: {}", err)),
                )
            })?;

        // Verify the proof
        let value = state_store
            .verify_proof(root_hash, &key, &proof_nodes)
            .map_err(|err| {
                RpcException::from(
                    RpcError::invalid_params()
                        .with_data(format!("Proof verification failed: {}", err)),
                )
            })?;

        Ok(Value::String(BASE64_STANDARD.encode(&value)))
    }

    /// `getstate` - Get state value from trie.
    ///
    /// Parameters:
    /// - `roothash` (string): State root hash (UInt256)
    /// - `scripthash` (string): Contract script hash (UInt160)
    /// - `key` (string): Storage key (base64-encoded)
    ///
    /// Returns:
    /// - Base64-encoded value
    ///
    /// Reference: C# Neo.Plugins.StateService RpcServer.GetState
    pub fn get_state(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let state_store = Self::get_state_store(server)?;

        let root_hash = Self::parse_uint256(params, 0, "roothash")?;
        let script_hash = Self::parse_uint160(params, 1, "scripthash")?;
        let key_base64 = Self::parse_string(params, 2, "key")?;

        let key_bytes = BASE64_STANDARD.decode(key_base64).map_err(|err| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("Invalid base64 key: {}", err)),
            )
        })?;

        // Resolve contract ID
        let store = server.system().store_cache();
        let contract_mgmt = ContractManagement::new();
        let contract = contract_mgmt
            .get_contract_by_hash(&store, &script_hash)
            .map_err(internal_error)?
            .ok_or_else(|| {
                RpcException::from(RpcError::invalid_params().with_data("Contract not found"))
            })?;

        let storage_key = StorageKey::new(contract.id, key_bytes);

        // Get snapshot for the root
        let snapshot = state_store.get_snapshot_by_root(root_hash).ok_or_else(|| {
            if !state_store.full_state_enabled() {
                RpcException::from(RpcError::unsupported_state())
            } else {
                RpcException::from(RpcError::unknown_state_root())
            }
        })?;

        // Get value from trie
        let value = snapshot
            .trie
            .try_get(&storage_key.as_bytes())
            .map_err(internal_error)?
            .unwrap_or_default();

        Ok(Value::String(BASE64_STANDARD.encode(&value)))
    }

    /// `findstates` - Find multiple state values by prefix.
    ///
    /// Parameters:
    /// - `roothash` (string): State root hash (UInt256)
    /// - `scripthash` (string): Contract script hash (UInt160)
    /// - `prefix` (string): Key prefix (base64-encoded, can be empty)
    /// - `key` (string, optional): Start key for pagination (base64-encoded)
    /// - `count` (number, optional): Maximum results (default 20, max 100)
    ///
    /// Returns:
    /// - Object with results array, firstProof, lastProof, and truncated flag
    ///
    /// Reference: C# Neo.Plugins.StateService RpcServer.FindStates
    pub fn find_states(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        let state_store = Self::get_state_store(server)?;

        let root_hash = Self::parse_uint256(params, 0, "roothash")?;
        let script_hash = Self::parse_uint160(params, 1, "scripthash")?;
        let prefix_base64 = Self::parse_string(params, 2, "prefix")?;

        let prefix_bytes = if prefix_base64.is_empty() {
            Vec::new()
        } else {
            BASE64_STANDARD.decode(prefix_base64).map_err(|err| {
                RpcException::from(
                    RpcError::invalid_params()
                        .with_data(format!("Invalid base64 prefix: {}", err)),
                )
            })?
        };

        let start_key_bytes = if let Some(start_base64) = params.get(3).and_then(|v| v.as_str()) {
            if start_base64.is_empty() {
                Vec::new()
            } else {
                BASE64_STANDARD.decode(start_base64).map_err(|err| {
                    RpcException::from(
                        RpcError::invalid_params()
                            .with_data(format!("Invalid base64 start key: {}", err)),
                    )
                })?
            }
        } else {
            Vec::new()
        };

        let count = params
            .get(4)
            .and_then(|v| v.as_u64())
            .and_then(|v| usize::try_from(v).ok())
            .unwrap_or(20)
            .min(100);

        // Resolve contract ID
        let store = server.system().store_cache();
        let contract_mgmt = ContractManagement::new();
        let contract = contract_mgmt
            .get_contract_by_hash(&store, &script_hash)
            .map_err(internal_error)?
            .ok_or_else(|| {
                RpcException::from(RpcError::invalid_params().with_data("Contract not found"))
            })?;

        // Get snapshot for the root
        let snapshot = state_store.get_snapshot_by_root(root_hash).ok_or_else(|| {
            if !state_store.full_state_enabled() {
                RpcException::from(RpcError::unsupported_state())
            } else {
                RpcException::from(RpcError::unknown_state_root())
            }
        })?;

        // Build search key: contract_id + prefix
        let mut search_prefix = Vec::with_capacity(4 + prefix_bytes.len());
        search_prefix.extend_from_slice(&contract.id.to_le_bytes());
        search_prefix.extend_from_slice(&prefix_bytes);

        let search_start = if start_key_bytes.is_empty() {
            search_prefix.clone()
        } else {
            let mut start = Vec::with_capacity(4 + start_key_bytes.len());
            start.extend_from_slice(&contract.id.to_le_bytes());
            start.extend_from_slice(&start_key_bytes);
            start
        };

        // Iterate through trie to find matching keys
        let mut results = Vec::new();
        let mut truncated = false;

        // Get all keys with the prefix
        let all_keys: Vec<(Vec<u8>, Vec<u8>)> = snapshot
            .trie
            .iter_from(&search_start)
            .map_err(internal_error)?
            .filter_map(|result| result.ok())
            .take_while(|(key, _)| key.starts_with(&search_prefix))
            .collect();

        truncated = all_keys.len() > count;
        let limited_keys: Vec<(Vec<u8>, Vec<u8>)> = all_keys.into_iter().take(count).collect();

        for (full_key, value) in &limited_keys {
            // Extract user key (skip contract ID)
            let user_key = if full_key.len() > 4 {
                &full_key[4..]
            } else {
                &[]
            };

            results.push(json!({
                "key": BASE64_STANDARD.encode(user_key),
                "value": BASE64_STANDARD.encode(value),
            }));
        }

        // Generate proofs for first and last keys
        let first_proof = if let Some((first_key, _)) = limited_keys.first() {
            let proof = snapshot
                .trie
                .try_get_proof(first_key)
                .map_err(internal_error)?
                .unwrap_or_default();
            let proof_nodes: Vec<Vec<u8>> = proof.into_iter().collect();
            let encoded = StateStore::encode_proof_payload(first_key, &proof_nodes);
            BASE64_STANDARD.encode(&encoded)
        } else {
            String::new()
        };

        let last_proof = if let Some((last_key, _)) = limited_keys.last() {
            let proof = snapshot
                .trie
                .try_get_proof(last_key)
                .map_err(internal_error)?
                .unwrap_or_default();
            let proof_nodes: Vec<Vec<u8>> = proof.into_iter().collect();
            let encoded = StateStore::encode_proof_payload(last_key, &proof_nodes);
            BASE64_STANDARD.encode(&encoded)
        } else {
            String::new()
        };

        Ok(json!({
            "firstProof": first_proof,
            "lastProof": last_proof,
            "truncated": truncated,
            "results": results,
        }))
    }

    // Helper methods

    fn get_state_store(server: &RpcServer) -> Result<std::sync::Arc<StateStore>, RpcException> {
        server
            .system()
            .get_named_service::<StateStore>(STATE_STORE_SERVICE)
            .map_err(internal_error)?
            .ok_or_else(|| {
                RpcException::from(
                    RpcError::method_not_found()
                        .with_data("State service not available"),
                )
            })
    }

    fn state_root_to_json(state_root: &StateRoot) -> Value {
        let mut obj = Map::new();
        obj.insert("version".to_string(), json!(state_root.version));
        obj.insert("index".to_string(), json!(state_root.index));
        obj.insert("roothash".to_string(), json!(state_root.root_hash.to_string()));

        // Serialize witness if present
        if let Some(witness) = &state_root.witness {
            let mut witness_obj = Map::new();
            witness_obj.insert(
                "invocation".to_string(),
                Value::String(BASE64_STANDARD.encode(&witness.invocation_script)),
            );
            witness_obj.insert(
                "verification".to_string(),
                Value::String(BASE64_STANDARD.encode(&witness.verification_script)),
            );
            obj.insert("witnesses".to_string(), json!([witness_obj]));
        } else {
            obj.insert("witnesses".to_string(), json!([]));
        }

        Value::Object(obj)
    }

    fn parse_uint256(
        params: &[Value],
        index: usize,
        name: &str,
    ) -> Result<UInt256, RpcException> {
        let value = params.get(index).and_then(|v| v.as_str()).ok_or_else(|| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("{} parameter required (UInt256)", name)),
            )
        })?;

        UInt256::from_str(value).map_err(|err| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("Invalid {}: {}", name, err)),
            )
        })
    }

    fn parse_uint160(
        params: &[Value],
        index: usize,
        name: &str,
    ) -> Result<neo_primitives::UInt160, RpcException> {
        let value = params.get(index).and_then(|v| v.as_str()).ok_or_else(|| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("{} parameter required (UInt160)", name)),
            )
        })?;

        neo_primitives::UInt160::from_str(value).map_err(|err| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("Invalid {}: {}", name, err)),
            )
        })
    }

    fn parse_string<'a>(
        params: &'a [Value],
        index: usize,
        name: &str,
    ) -> Result<&'a str, RpcException> {
        params.get(index).and_then(|v| v.as_str()).ok_or_else(|| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("{} parameter required (string)", name)),
            )
        })
    }
}

#[cfg(test)]
mod tests;
