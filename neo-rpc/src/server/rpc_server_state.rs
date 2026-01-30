//! State service RPC endpoints (parity with C# `StateService` RPC plugin).

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_helpers::internal_error;
use crate::server::rpc_method_attribute::RpcMethodDescriptor;
use crate::server::rpc_server::{RpcHandler, RpcServer};
use base64::engine::general_purpose::STANDARD as BASE64_STANDARD;
use base64::Engine as _;
use neo_core::neo_system::STATE_STORE_SERVICE;
use neo_core::smart_contract::native::contract_management::ContractManagement;
use neo_core::smart_contract::native::NativeContract;
use neo_core::smart_contract::storage_key::StorageKey;
use neo_core::smart_contract::StorageItem;
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
            .map_err(internal_error)?;
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
            "validatedrootindex": state_store.validated_root_index(),
        }))
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

        let proof_bytes = StateStore::encode_proof_payload(&storage_key.to_array(), &proof_nodes);
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
            .get(&storage_key.to_array())
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
            Some(Value::String(s)) if !s.is_empty() => {
                Some(BASE64_STANDARD.decode(s).map_err(|_| {
                    RpcException::from(
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
                    RpcException::from(
                        RpcError::invalid_params()
                            .with_data("findstates count must be a non-negative integer"),
                    )
                })?,
            _ => 0,
        };
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
                &storage_key.to_array(),
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
        let value = params.get(idx).and_then(Value::as_str).ok_or_else(|| {
            RpcException::from(
                RpcError::invalid_params()
                    .with_data(format!("{method} expects {descriptor} at index {idx}")),
            )
        })?;
        BASE64_STANDARD.decode(value).map_err(|_| {
            RpcException::from(
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
            .get(&storage_key.to_array())
            .map_err(|e| {
                RpcException::from(RpcError::internal_server_error().with_data(e.to_string()))
            })?
            .ok_or_else(|| RpcException::from(RpcError::unknown_contract()))?;
        let mut item = StorageItem::new();
        item.deserialize_from_bytes(&value);
        let contract = ContractManagement::deserialize_contract_state(&item.get_value())
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
mod tests {
    use super::*;
    use neo_core::neo_io::BinaryWriter;
    use neo_core::persistence::StorageItem;
    use neo_core::smart_contract::manifest::ContractManifest;
    use neo_core::smart_contract::native::NativeRegistry;
    use neo_core::smart_contract::ContractState;
    use neo_core::state_service::state_store::MemoryStateStoreBackend;
    use neo_core::state_service::state_store::StateServiceSettings;
    use neo_core::{NeoSystem, ProtocolSettings};

    fn make_server_with_state(full_state: bool) -> (Arc<NeoSystem>, Arc<StateStore>, RpcServer) {
        let settings = ProtocolSettings::default();
        let system = NeoSystem::new_with_state_service(
            settings,
            None,
            None,
            Some(StateServiceSettings {
                full_state,
                ..StateServiceSettings::default()
            }),
        )
        .expect("NeoSystem::new_with_state_service should succeed");

        let state_store = Arc::new(StateStore::new(
            Arc::new(MemoryStateStoreBackend::new()),
            StateServiceSettings {
                full_state,
                ..StateServiceSettings::default()
            },
        ));
        system
            .add_named_service::<StateStore, _>(STATE_STORE_SERVICE, state_store.clone())
            .expect("override state store service");

        let mut server = RpcServer::new(system.clone(), Default::default());
        server.register_handlers(RpcServerState::register_handlers());

        (system, state_store, server)
    }

    fn store_contract_state(
        store: &mut neo_core::persistence::StoreCache,
        contract: &ContractState,
    ) -> (StorageKey, Vec<u8>) {
        const PREFIX_CONTRACT: u8 = 0x08;
        const PREFIX_CONTRACT_HASH: u8 = 0x0c;

        let contract_mgmt_id = NativeRegistry::new()
            .get_by_name("ContractManagement")
            .expect("contract management")
            .id();

        let mut writer = BinaryWriter::new();
        contract.serialize(&mut writer).expect("serialize contract");
        let contract_bytes = writer.into_bytes();

        let mut key_bytes = Vec::with_capacity(1 + 20);
        key_bytes.push(PREFIX_CONTRACT);
        key_bytes.extend_from_slice(&contract.hash.to_bytes());
        let key = StorageKey::new(contract_mgmt_id, key_bytes);
        store.add(key.clone(), StorageItem::from_bytes(contract_bytes.clone()));

        let mut id_bytes = Vec::with_capacity(1 + 4);
        id_bytes.push(PREFIX_CONTRACT_HASH);
        id_bytes.extend_from_slice(&contract.id.to_be_bytes());
        let id_key = StorageKey::new(contract_mgmt_id, id_bytes);
        store.add(
            id_key,
            StorageItem::from_bytes(contract.hash.to_bytes().to_vec()),
        );

        let mut legacy_bytes = Vec::with_capacity(1 + 4);
        legacy_bytes.push(PREFIX_CONTRACT_HASH);
        legacy_bytes.extend_from_slice(&contract.id.to_le_bytes());
        let legacy_key = StorageKey::new(contract_mgmt_id, legacy_bytes);
        store.add(
            legacy_key,
            StorageItem::from_bytes(contract.hash.to_bytes().to_vec()),
        );

        store.commit();
        (key, contract_bytes)
    }

    fn seed_state_root(
        state_store: &StateStore,
        index: u32,
        entries: Vec<(StorageKey, Vec<u8>)>,
    ) -> UInt256 {
        let mut snapshot = state_store.get_snapshot();
        for (key, value) in entries {
            snapshot
                .trie
                .put(&key.to_array(), &value)
                .expect("trie put");
        }
        let root_hash = snapshot.trie.root_hash().unwrap_or_else(UInt256::zero);
        let state_root = StateRoot::new_current(index, root_hash);
        snapshot
            .add_local_state_root(&state_root)
            .expect("add local state root");
        snapshot.commit().expect("commit snapshot");
        root_hash
    }

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

    #[tokio::test(flavor = "multi_thread")]
    async fn state_height_and_root_handlers_work() {
        let (_system, state_store, server) = make_server_with_state(true);

        // Seed a local state root at height 1 via snapshot helpers so current snapshot updates.
        state_store.update_local_state_root_snapshot(1, std::iter::empty());
        state_store.update_local_state_root(1);
        let root_hash = state_store
            .current_local_root_hash()
            .unwrap_or_else(UInt256::zero);

        // getstateheight
        let height = RpcServerState::get_state_height(&server, &[])
            .expect("state height")
            .as_object()
            .cloned()
            .expect("object");
        assert_eq!(
            height.get("localrootindex").and_then(Value::as_u64),
            Some(1)
        );

        // getstateroot
        let root_obj = RpcServerState::get_state_root(&server, &[Value::Number(1u64.into())])
            .expect("state root")
            .as_object()
            .cloned()
            .expect("object");
        assert_eq!(
            root_obj
                .get("roothash")
                .and_then(Value::as_str)
                .expect("roothash"),
            root_hash.to_string()
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn state_queries_reject_old_roots_when_full_state_disabled() {
        let (_system, state_store, server) = make_server_with_state(false);

        // Install a state store with FullState disabled and seed a current local root at height 1.
        state_store.update_local_state_root_snapshot(1, std::iter::empty());
        state_store.update_local_state_root(1);

        // Querying a non-current root should return UnsupportedState when FullState is false.
        let old_root = UInt256::from_bytes(&[1u8; 32]).expect("old root hash");
        let params = vec![
            Value::String(old_root.to_string()),
            Value::String("0x0000000000000000000000000000000000000000".to_string()),
            Value::String(BASE64_STANDARD.encode([0u8])),
        ];
        let err = RpcServerState::get_state(&server, &params).unwrap_err();
        assert_eq!(err.code(), RpcError::unsupported_state().code());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn proof_handlers_round_trip_value() {
        let (_system, state_store, server) = make_server_with_state(true);

        // Seed a storage item in the trie and materialize a state root at height 1.
        let user_key = vec![0x01u8];
        let storage_key = StorageKey::new(1, user_key.clone());
        let value = vec![0xBA, 0xAD, 0xF0, 0x0D];
        let mut snapshot = state_store.get_snapshot();
        snapshot
            .trie
            .put(&storage_key.to_array(), &value)
            .expect("trie put");
        let root_hash = snapshot
            .trie
            .root_hash()
            .expect("root hash should compute after put");
        let state_root = StateRoot::new_current(1, root_hash);
        snapshot
            .add_local_state_root(&state_root)
            .expect("add local state root");
        snapshot.commit().expect("commit snapshot");

        // Build a proof directly from the snapshot and verify it via the handler.
        let proof_nodes: Vec<Vec<u8>> = snapshot
            .trie
            .try_get_proof(&storage_key.to_array())
            .expect("proof exists")
            .expect("proof set present")
            .into_iter()
            .collect();
        let encoded_proof = StateStore::encode_proof_payload(&storage_key.to_array(), &proof_nodes);
        let decoded_value = RpcServerState::verify_proof(
            &server,
            &[
                Value::String(root_hash.to_string()),
                Value::String(BASE64_STANDARD.encode(&encoded_proof)),
            ],
        )
        .expect("proof verification");
        let decoded = BASE64_STANDARD
            .decode(decoded_value.as_str().expect("verify returns string"))
            .expect("base64 decode");
        assert_eq!(decoded, value);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn state_root_rejects_unknown_index() {
        let (_system, _state_store, server) = make_server_with_state(true);
        let err = RpcServerState::get_state_root(&server, &[Value::Number(999u64.into())])
            .expect_err("unknown state root");
        assert_eq!(err.code(), RpcError::unknown_state_root().code());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_proof_rejects_invalid_key() {
        let (_system, _state_store, server) = make_server_with_state(true);
        let params = vec![
            Value::String(UInt256::zero().to_string()),
            Value::String(UInt160::zero().to_string()),
            Value::String("invalid_base64".to_string()),
        ];
        let err = RpcServerState::get_proof(&server, &params).expect_err("invalid key");
        assert_eq!(err.code(), RpcError::invalid_params().code());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn verify_proof_rejects_invalid_payload() {
        let (_system, _state_store, server) = make_server_with_state(true);
        let params = vec![
            Value::String(UInt256::zero().to_string()),
            Value::String("invalid_proof".to_string()),
        ];
        let err = RpcServerState::verify_proof(&server, &params).expect_err("invalid proof");
        assert_eq!(err.code(), RpcError::invalid_params().code());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn get_state_returns_value_from_trie() {
        let (system, state_store, server) = make_server_with_state(true);
        let script_hash = UInt160::from_bytes(&[0x11u8; 20]).expect("script hash");

        let contract = ContractState::new(
            1,
            script_hash,
            neo_core::smart_contract::NefFile::new("test".to_string(), vec![0x01]),
            ContractManifest::default(),
        );
        let mut store = system.context().store_snapshot_cache();
        let (contract_key, contract_bytes) = store_contract_state(&mut store, &contract);

        let storage_key = StorageKey::new(1, vec![0x01, 0x02]);
        let value = vec![0xaa, 0xbb];
        let root_hash = seed_state_root(
            state_store.as_ref(),
            1,
            vec![(contract_key, contract_bytes), (storage_key, value.clone())],
        );

        let params = vec![
            Value::String(root_hash.to_string()),
            Value::String(script_hash.to_string()),
            Value::String(BASE64_STANDARD.encode([0x01u8, 0x02u8])),
        ];
        let result = RpcServerState::get_state(&server, &params)
            .expect("state value")
            .as_str()
            .map(|s| BASE64_STANDARD.decode(s).expect("base64 decode"))
            .expect("string");
        assert_eq!(result, value);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn find_states_returns_results() {
        let (system, state_store, server) = make_server_with_state(true);
        let script_hash = UInt160::from_bytes(&[0x22u8; 20]).expect("script hash");

        let contract = ContractState::new(
            1,
            script_hash,
            neo_core::smart_contract::NefFile::new("test".to_string(), vec![0x01]),
            ContractManifest::default(),
        );
        let mut store = system.context().store_snapshot_cache();
        let (contract_key, contract_bytes) = store_contract_state(&mut store, &contract);

        let key1 = StorageKey::new(1, vec![0x01, 0x02]);
        let key2 = StorageKey::new(1, vec![0x03, 0x04]);
        let root_hash = seed_state_root(
            state_store.as_ref(),
            1,
            vec![
                (contract_key, contract_bytes),
                (key1, vec![0xaa, 0xbb]),
                (key2, vec![0xcc, 0xdd]),
            ],
        );

        let params = vec![
            Value::String(root_hash.to_string()),
            Value::String(script_hash.to_string()),
            Value::String(String::new()),
        ];
        let result = RpcServerState::find_states(&server, &params)
            .expect("find states")
            .as_object()
            .cloned()
            .expect("object");
        let results = result
            .get("results")
            .and_then(Value::as_array)
            .expect("results array");
        assert_eq!(results.len(), 2);

        let first_key = results[0]
            .get("key")
            .and_then(Value::as_str)
            .map(|s| BASE64_STANDARD.decode(s).expect("base64 decode"))
            .expect("key");
        let first_value = results[0]
            .get("value")
            .and_then(Value::as_str)
            .map(|s| BASE64_STANDARD.decode(s).expect("base64 decode"))
            .expect("value");
        let second_key = results[1]
            .get("key")
            .and_then(Value::as_str)
            .map(|s| BASE64_STANDARD.decode(s).expect("base64 decode"))
            .expect("key");
        let second_value = results[1]
            .get("value")
            .and_then(Value::as_str)
            .map(|s| BASE64_STANDARD.decode(s).expect("base64 decode"))
            .expect("value");

        assert_eq!(first_key, vec![0x01, 0x02]);
        assert_eq!(first_value, vec![0xaa, 0xbb]);
        assert_eq!(second_key, vec![0x03, 0x04]);
        assert_eq!(second_value, vec![0xcc, 0xdd]);
        assert_eq!(result.get("truncated"), Some(&Value::Bool(false)));
    }
}
