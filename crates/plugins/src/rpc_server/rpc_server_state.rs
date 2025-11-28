//! State service RPC endpoints (parity with C# StateService RPC plugin).

use crate::rpc_server::rpc_error::RpcError;
use crate::rpc_server::rpc_exception::RpcException;
use crate::rpc_server::rpc_method_attribute::RpcMethodDescriptor;
use crate::rpc_server::rpc_server::{RpcHandler, RpcServer};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use neo_core::neo_io::{BinaryWriter, MemoryReader};
use neo_core::neo_system::STATE_STORE_SERVICE;
use neo_core::smart_contract::native::contract_management::ContractManagement;
use neo_core::smart_contract::storage_key::StorageKey;
use neo_core::state_service::{StateRoot, StateStore};
use neo_core::{UInt160, UInt256};
use serde_json::{json, Map, Value};
use std::sync::Arc;

pub struct RpcServerState;

impl RpcServerState {
    pub fn register_handlers() -> Vec<RpcHandler> {
        vec![
            Self::handler("getstateheight", Self::get_state_height),
            Self::handler("getstateroot", Self::get_state_root),
            Self::handler("getproof", Self::get_proof),
            Self::handler("verifyproof", Self::verify_proof),
            Self::handler("getstate", Self::get_state),
            Self::handler("findstates", Self::find_states),
        ]
    }

    fn handler(
        name: &'static str,
        func: fn(&RpcServer, &[Value]) -> Result<Value, RpcException>,
    ) -> RpcHandler {
        RpcHandler::new(RpcMethodDescriptor::new(name), Arc::new(func))
    }

    fn state_store(server: &RpcServer) -> Result<Arc<StateStore>, RpcException> {
        let lookup = server
            .system()
            .state_store()
            .map_err(Self::internal_error)?;
        lookup.ok_or_else(|| {
            RpcException::new(
                RpcError::internal_server_error()
                    .with_data(format!("{STATE_STORE_SERVICE} service not registered")),
            )
        })
    }

    fn get_state_height(server: &RpcServer, _params: &[Value]) -> Result<Value, RpcException> {
        let state_store = Self::state_store(server)?;
        Ok(json!({
            "localrootindex": state_store.local_root_index(),
            "validatedrootindex": state_store.validated_root_index(),
        }))
    }

    fn get_state_root(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let index = Self::expect_u32(params, 0, "getstateroot")?;
        let state_store = Self::state_store(server)?;
        let state_root = state_store
            .get_state_root(index)
            .ok_or_else(|| RpcException::new(RpcError::unknown_state_root()))?;
        Ok(Self::state_root_to_json(&state_root))
    }

    fn get_proof(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let root_hash = Self::parse_uint256(params, 0, "getproof")?;
        let script_hash = Self::parse_uint160(params, 1, "getproof")?;
        let key = Self::parse_base64(params, 2, "getproof", "Base64 storage key")?;

        let state_store = Self::state_store(server)?;
        let contract_id = Self::resolve_contract_id(server, &script_hash)?;
        let storage_key = StorageKey::new(contract_id, key);
        let proof_nodes = state_store
            .get_proof(root_hash, &storage_key)
            .ok_or_else(|| RpcException::new(RpcError::unknown_storage_item()))?;

        let proof_bytes = StateStore::encode_proof_payload(&storage_key.to_array(), &proof_nodes);
        Ok(Value::String(BASE64_STANDARD.encode(proof_bytes)))
    }

    fn verify_proof(_server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let root_hash = Self::parse_uint256(params, 0, "verifyproof")?;
        let proof_bytes = Self::parse_base64(params, 1, "verifyproof", "Base64 proof payload")?;
        let (key, nodes) = StateStore::decode_proof_payload(&proof_bytes).ok_or_else(|| {
            RpcException::new(RpcError::invalid_params().with_data("invalid proof payload"))
        })?;
        let value = StateStore::verify_proof(root_hash, &key, &nodes).ok_or_else(|| {
            RpcException::new(
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
        let contract_id = Self::resolve_contract_id(server, &script_hash)?;
        let mut trie = state_store.trie_for_root(root_hash);
        let storage_key = StorageKey::new(contract_id, key);
        let value = trie
            .get(&storage_key.to_array())
            .map_err(|e| {
                RpcException::new(RpcError::internal_server_error().with_data(e.to_string()))
            })?
            .ok_or_else(|| RpcException::new(RpcError::unknown_storage_item()))?;

        Ok(Value::String(BASE64_STANDARD.encode(value)))
    }

    fn find_states(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        let root_hash = Self::parse_uint256(params, 0, "findstates")?;
        let script_hash = Self::parse_uint160(params, 1, "findstates")?;
        let prefix =
            Self::parse_base64(params, 2, "findstates", "Base64 prefix for storage search")?;
        let from = match params.get(3) {
            Some(Value::String(s)) if !s.is_empty() => {
                Some(BASE64_STANDARD.decode(s).map_err(|_| {
                    RpcException::new(
                        RpcError::invalid_params()
                            .with_data("findstates expects Base64-encoded 'from' parameter"),
                    )
                })?)
            }
            _ => None,
        };
        let count = match params.get(4) {
            Some(Value::Number(n)) => n
                .as_u64()
                .and_then(|v| usize::try_from(v).ok())
                .ok_or_else(|| {
                    RpcException::new(
                        RpcError::invalid_params()
                            .with_data("findstates count must be a non-negative integer"),
                    )
                })?,
            _ => server.settings().find_storage_page_size,
        };

        let state_store = Self::state_store(server)?;
        let contract_id = Self::resolve_contract_id(server, &script_hash)?;
        let mut trie = state_store.trie_for_root(root_hash);
        let search_prefix = StorageKey::create_search_prefix(contract_id, &prefix);
        let from_key = from
            .as_ref()
            .map(|suffix| StorageKey::create_search_prefix(contract_id, suffix));

        let entries = trie
            .find(&search_prefix, from_key.as_deref())
            .map_err(|e| {
                RpcException::new(RpcError::internal_server_error().with_data(e.to_string()))
            })?;

        let mut truncated = false;
        let mut results = Vec::new();
        for entry in entries.into_iter() {
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
            let storage_key = StorageKey::new(contract_id, key.clone());
            state_store.get_proof(root_hash, &storage_key).map(|proof| {
                BASE64_STANDARD.encode(StateStore::encode_proof_payload(
                    &storage_key.to_array(),
                    &proof,
                ))
            })
        });
        let last_proof = results.last().and_then(|(key, _)| {
            let storage_key = StorageKey::new(contract_id, key.clone());
            state_store.get_proof(root_hash, &storage_key).map(|proof| {
                BASE64_STANDARD.encode(StateStore::encode_proof_payload(
                    &storage_key.to_array(),
                    &proof,
                ))
            })
        });

        let serialized_results: Vec<Value> = results
            .into_iter()
            .map(|(key, value)| {
                json!({
                    "key": BASE64_STANDARD.encode(key),
                    "value": BASE64_STANDARD.encode(value),
                })
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

    fn parse_uint256(params: &[Value], idx: usize, method: &str) -> Result<UInt256, RpcException> {
        let value = params.get(idx).and_then(Value::as_str).ok_or_else(|| {
            RpcException::new(
                RpcError::invalid_params()
                    .with_data(format!("{method} expects UInt256 parameter at index {idx}")),
            )
        })?;
        UInt256::parse(value).map_err(|_| {
            RpcException::new(
                RpcError::invalid_params().with_data("failed to parse UInt256 parameter"),
            )
        })
    }

    fn parse_uint160(params: &[Value], idx: usize, method: &str) -> Result<UInt160, RpcException> {
        let value = params.get(idx).and_then(Value::as_str).ok_or_else(|| {
            RpcException::new(
                RpcError::invalid_params()
                    .with_data(format!("{method} expects UInt160 parameter at index {idx}")),
            )
        })?;
        UInt160::parse(value).map_err(|_| {
            RpcException::new(
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
        let value = params.get(idx).and_then(Value::as_str).ok_or_else(|| {
            RpcException::new(
                RpcError::invalid_params()
                    .with_data(format!("{method} expects {descriptor} at index {idx}")),
            )
        })?;
        BASE64_STANDARD.decode(value).map_err(|_| {
            RpcException::new(
                RpcError::invalid_params()
                    .with_data(format!("{method} expects {descriptor} at index {idx}")),
            )
        })
    }

    fn expect_u32(params: &[Value], idx: usize, method: &str) -> Result<u32, RpcException> {
        params
            .get(idx)
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .ok_or_else(|| {
                RpcException::new(
                    RpcError::invalid_params()
                        .with_data(format!("{method} expects unsigned integer parameter")),
                )
            })
    }

    fn resolve_contract_id(server: &RpcServer, hash: &UInt160) -> Result<i32, RpcException> {
        let store = server.system().store_cache();
        let contract = ContractManagement::get_contract_from_store_cache(&store, hash)
            .map_err(Self::internal_error)?
            .ok_or_else(|| RpcException::new(RpcError::unknown_contract()))?;
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
        if let Some(witness) = &root.witness {
            let mut witness_obj = Map::new();
            witness_obj.insert(
                "invocation".to_string(),
                Value::String(BASE64_STANDARD.encode(witness.invocation_script())),
            );
            witness_obj.insert(
                "verification".to_string(),
                Value::String(BASE64_STANDARD.encode(witness.verification_script())),
            );
            obj.insert(
                "witnesses".to_string(),
                Value::Array(vec![Value::Object(witness_obj)]),
            );
        }
        Value::Object(obj)
    }

    fn internal_error<T: ToString>(err: T) -> RpcException {
        RpcException::new(RpcError::internal_server_error().with_data(err.to_string()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_round_trips() {
        let key = vec![1, 2, 3, 4];
        let nodes = vec![vec![0xAA, 0xBB], vec![0xCC]];
        let encoded = StateStore::encode_proof_payload(&key, &nodes);
        let (decoded_key, decoded_nodes) =
            StateStore::decode_proof_payload(&encoded).expect("proof decode");
        assert_eq!(decoded_key, key);
        assert_eq!(decoded_nodes, nodes);
    }
}
