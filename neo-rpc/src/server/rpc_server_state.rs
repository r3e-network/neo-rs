//! State service RPC endpoints (parity with C# `StateService` RPC plugin).

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::{
    decode_base64_text, expect_base64_param_with_message, internal_error};
use crate::server::rpc_server::{RpcHandler, RpcServer};
use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
// STATE_STORE_SERVICE moved;
use neo_storage::StorageItem;
use neo_native_contracts::NativeContract;
use neo_native_contracts::contract_management::ContractManagement;
use neo_storage::StorageKey;
use neo_state_service::{StateRoot, StateStore};
use neo_primitives::{UInt160, UInt256};
use serde_json::{Map, Value, json};
use std::sync::Arc;

pub struct RpcServerState;

impl RpcServerState {
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
        let lookup = server.system().state_store().map_err(internal_error)?;
        lookup.ok_or_else(|| {
            RpcException::from(
                RpcError::internal_server_error()
                    .with_data(format!("{STATE_STORE_SERVICE} service not registered")),
            )
       })
   }

    fn get_state_height(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let state_store = Self::state_store(server)?;
        Ok(json!({
            "localrootindex": state_store.local_root_index(),
            "validatedrootindex": state_store.validated_root_index()}))
   }

    fn get_state_root(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let index = Self::expect_u32(params, 0, "getstateroot")?;
        let state_store = Self::state_store(server)?;
        let state_root = state_store
            .get_state_root(index)
            .ok_or_else(|| RpcException::from(RpcError::unknown_state_root()))?;
        Ok(Self::state_root_to_json(&state_root))
   }

    fn get_proof(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let root_hash = Self::parse_uint256(params, 0, "getproof")?;
        let script_hash = Self::parse_uint160(params, 1, "getproof")?;
        let key = Self::parse_base64(params, 2, "getproof", "Base64 storage key")?;

        let state_store = Self::state_store(server)?;
        Self::check_root_hash(&state_store, root_hash)?;
        let contract_id =
            Self::resolve_contract_id_for_root(&state_store, root_hash, &script_hash)?;
        let storage_key = StorageKey::new(contract_id, key);
        let proof_nodes = state_store
            .get_proof(root_hash, &storage_key)
            .ok_or_else(|| RpcException::from(RpcError::unknown_storage_item()))?;

        let proof_bytes = StateStore::encode_proof_payload(&storage_key.as_bytes(), &proof_nodes);
        Ok(Value::String(BASE64_STANDARD.encode(proof_bytes)))
   }

    fn verify_proof(_server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let root_hash = Self::parse_uint256(params, 0, "verifyproof")?;
        let proof_bytes = Self::parse_base64(params, 1, "verifyproof", "Base64 proof payload")?;
        let (key, nodes) = StateStore::decode_proof_payload(&proof_bytes).ok_or_else(|| {
            RpcException::from(RpcError::invalid_params().with_data("invalid proof payload"))
       })?;
        let value = StateStore::verify_proof(root_hash, &key, &nodes).ok_or_else(|| {
            RpcException::from(
                RpcError::verification_failed()
                    .with_data("failed to verify state proof against supplied root"),
            )
       })?;
        Ok(Value::String(BASE64_STANDARD.encode(value)))
   }

    fn get_state(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let root_hash = Self::parse_uint256(params, 0, "getstate")?;
        let script_hash = Self::parse_uint160(params, 1, "getstate")?;
        let key = Self::parse_base64(params, 2, "getstate", "Base64 storage key")?;
        let state_store = Self::state_store(server)?;
        Self::check_root_hash(&state_store, root_hash)?;
        let contract_id =
            Self::resolve_contract_id_for_root(&state_store, root_hash, &script_hash)?;
        let mut trie = state_store.trie_for_root(root_hash);
        let storage_key = StorageKey::new(contract_id, key);
        let value = trie
            .get(&storage_key.as_bytes())
            .map_err(|e| {
                RpcException::from(RpcError::internal_server_error().with_data(e.to_string()))
           })?
            .ok_or_else(|| RpcException::from(RpcError::unknown_storage_item()))?;

        Ok(Value::String(BASE64_STANDARD.encode(value)))
   }

    fn find_states(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let root_hash = Self::parse_uint256(params, 0, "findstates")?;
        let script_hash = Self::parse_uint160(params, 1, "findstates")?;
        let prefix =
            Self::parse_base64(params, 2, "findstates", "Base64 prefix for storage search")?;
        let from = match params.get(3) {
            Some(Value::String(s)) if !s.is_empty() => Some(decode_base64_text(
                s,
                "findstates expects Base64-encoded 'from' parameter",
            )?),
            _ => None};
        let count = match params.get(4) {
            Some(Value::Number(n)) => n
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| {
                    RpcException::from(
                        RpcError::invalid_params()
                            .with_data("findstates count must be a non-negative integer"),
                    )
               })?,
            _ => 0};
        let state_store = Self::state_store(server)?;
        let max_count = state_store.max_find_result_items();
        let count = if count == 0 {
            max_count
       } else {
            count.min(max_count)
       };
        Self::check_root_hash(&state_store, root_hash)?;
        let contract_id =
            Self::resolve_contract_id_for_root(&state_store, root_hash, &script_hash)?;
        let mut trie = state_store.trie_for_root(root_hash);
        let search_prefix = StorageKey::create_search_prefix(contract_id, &prefix);
        let from_key = from
            .as_ref()
            .map(|suffix| StorageKey::create_search_prefix(contract_id, suffix));

        let entries = trie
            .find(&search_prefix, from_key.as_deref())
            .map_err(|e| {
                RpcException::from(RpcError::internal_server_error().with_data(e.to_string()))
           })?;

        let mut truncated = false;
        let mut results = Vec::new();
        for entry in entries {
            if results.len() >= count {
                truncated = true;
                break;
           }

            if entry.key.len() < std::mem::size_of::<i32>() {
                continue;
           }

            let suffix = &entry.key[std::mem::size_of::<i32>()..];
            results.push((suffix.to_vec(), entry.value));
       }

        let first_proof = results.first().and_then(|(key, _)| {
            Self::encode_proof_base64(&state_store, root_hash, contract_id, key)
       });
        let last_proof = if results.len() > 1 {
            results.last().and_then(|(key, _)| {
                Self::encode_proof_base64(&state_store, root_hash, contract_id, key)
           })
       } else {
            None
       };

        let serialized_results: Vec<Value> = results
            .into_iter()
            .map(|(key, value)| {
                json!({
                    "key": BASE64_STANDARD.encode(key),
                    "value": BASE64_STANDARD.encode(value)})
           })
            .collect();

        let mut response = Map::new();
        response.insert("truncated".to_string(), Value::Bool(truncated));
        response.insert("results".to_string(), Value::Array(serialized_results));
        if let Some(proof) = first_proof {
            response.insert("firstProof".to_string(), Value::String(proof));
       }
        if let Some(proof) = last_proof {
            response.insert("lastProof".to_string(), Value::String(proof));
       }

        Ok(Value::Object(response))
   }

    fn check_root_hash(
        state_store: &Arc<StateStore>,
        root_hash: UInt256,
    ) -> Result<(), RpcException> {
        if state_store.full_state() {
            return Ok(());
       }

        let current = state_store.current_local_root_hash();
        if current != Some(root_hash) {
            return Err(RpcException::from(RpcError::unsupported_state().with_data(
                format!(
                    "fullState:false,current:{},rootHash:{}",
                    current.map_or_else(|| "<none>".to_string(), |h| h.to_string()),
                    root_hash
                ),
            )));
       }
        Ok(())
   }

    fn encode_proof_base64(
        state_store: &Arc<StateStore>,
        root_hash: UInt256,
        contract_id: i32,
        key: &[u8],
    ) -> Option<String> {
        let storage_key = StorageKey::new(contract_id, key.to_vec());
        state_store.get_proof(root_hash, &storage_key).map(|proof| {
            BASE64_STANDARD.encode(StateStore::encode_proof_payload(
                &storage_key.as_bytes(),
                &proof,
            ))
       })
   }

    fn parse_uint256(params: &[Value], idx: usize, method: &str) -> Result<UInt256, RpcException> {
        let value = params.get(idx).and_then(Value::as_str).ok_or_else(|| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("{method} expects UInt256 parameter at index {idx}")),
            )
       })?;
        UInt256::parse(value).map_err(|_| {
            RpcException::from(
                RpcError::invalid_params().with_data("failed to parse UInt256 parameter"),
            )
       })
   }

    fn parse_uint160(params: &[Value], idx: usize, method: &str) -> Result<UInt160, RpcException> {
        let value = params.get(idx).and_then(Value::as_str).ok_or_else(|| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("{method} expects UInt160 parameter at index {idx}")),
            )
       })?;
        UInt160::parse(value).map_err(|_| {
            RpcException::from(
                RpcError::invalid_params().with_data("failed to parse UInt160 parameter"),
            )
       })
   }

    fn parse_base64(
        params: &[Value],
        idx: usize,
        method: &str,
        descriptor: &str,
    ) -> Result<Vec<u8>, RpcException> {
        expect_base64_param_with_message(
            params,
            idx,
            format!("{method} expects {descriptor} at index {idx}"),
        )
   }

    fn expect_u32(params: &[Value], idx: usize, method: &str) -> Result<u32, RpcException> {
        params
            .get(idx)
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| {
                RpcException::from(
                    RpcError::invalid_params()
                        .with_data(format!("{method} expects unsigned integer parameter")),
                )
           })
   }

    fn resolve_contract_id_for_root(
        state_store: &Arc<StateStore>,
        root_hash: UInt256,
        hash: &UInt160,
    ) -> Result<i32, RpcException> {
        let contract_mgmt = ContractManagement::new();
        let storage_key = StorageKey::new(
            contract_mgmt.id(),
            ContractManagement::contract_storage_key(hash),
        );
        let mut trie = state_store.trie_for_root(root_hash);
        let value = trie
            .get(&storage_key.as_bytes())
            .map_err(|e| {
                RpcException::from(RpcError::internal_server_error().with_data(e.to_string()))
           })?
            .ok_or_else(|| RpcException::from(RpcError::unknown_contract()))?;
        let mut item = StorageItem::new();
        item.deserialize_from_bytes(&value);
        let contract = ContractManagement::deserialize_contract_state(&item.value_bytes())
            .map_err(internal_error)?;
        Ok(contract.id)
   }

    fn state_root_to_json(root: &StateRoot) -> Value {
        let mut obj = Map::new();
        obj.insert("version".to_string(), json!(root.version));
        obj.insert("index".to_string(), json!(root.index));
        obj.insert(
            "roothash".to_string(),
            Value::String(root.root_hash.to_string()),
        );
        let witnesses = if let Some(witness) = &root.witness {
            let mut witness_obj = Map::new();
            witness_obj.insert(
                "invocation".to_string(),
                Value::String(BASE64_STANDARD.encode(witness.invocation_script())),
            );
            witness_obj.insert(
                "verification".to_string(),
                Value::String(BASE64_STANDARD.encode(witness.verification_script())),
            );
            vec![Value::Object(witness_obj)]
       } else {
            Vec::new()
       };
        obj.insert("witnesses".to_string(), Value::Array(witnesses));
        Value::Object(obj)
   }
}

#[cfg(test)]
mod tests;
