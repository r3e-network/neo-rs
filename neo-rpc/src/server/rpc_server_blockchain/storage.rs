//! Contract-state and storage RPC handlers.
//!
//! These handlers stay in the blockchain RPC group because they are part of the
//! Neo JSON-RPC blockchain surface, but their parsing and storage iteration
//! dependencies are isolated from the block and transaction handlers.

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_storage::StorageKey;
use neo_storage::persistence::SeekDirection;
use serde_json::{Value, json};

use super::RpcServerBlockchain;
use super::request_helpers::{FindStorageRequest, GetStorageRequest};
use super::responses::contract_state_to_json;

impl RpcServerBlockchain {
    pub(super) fn get_contract_state(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getcontractstate", params)
                .map_err(RpcException::from);
        }
        let identifier = Self::parse_contract_identifier(params, "getcontractstate")?;
        let store = server.system().store_cache();
        let contract = Self::load_contract_state(&store, &identifier)?
            .ok_or_else(|| RpcException::from(RpcError::unknown_contract()))?;
        Ok(contract_state_to_json(&contract))
    }

    pub(super) fn get_storage(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("getstorage", params)
                .map_err(RpcException::from);
        }
        let request = GetStorageRequest::parse(params)?;

        let store = server.system().store_cache();
        let contract_id = Self::resolve_contract_id(&store, &request.identifier)?;
        let storage_key = StorageKey::new(contract_id, request.key_bytes);
        let value = store
            .get(&storage_key)
            .ok_or_else(|| RpcException::from(RpcError::unknown_storage_item()))?;
        Ok(Value::String(BASE64_STANDARD.encode(&*value.value_bytes())))
    }

    pub(super) fn find_storage(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        if let Some(remote) = server.remote_ledger_rpc() {
            return remote
                .call("findstorage", params)
                .map_err(RpcException::from);
        }
        let request = FindStorageRequest::parse(params)?;

        let store = server.system().store_cache();
        let contract_id = Self::resolve_contract_id(&store, &request.identifier)?;
        let prefix_key = StorageKey::new(contract_id, request.prefix_bytes);
        let iter = store.find(Some(&prefix_key), SeekDirection::Forward);

        let mut results = Vec::new();
        let mut skipped = 0usize;
        let mut truncated = false;
        let page_size = server.settings().find_storage_page_size;
        for (key, value) in iter {
            if key.id != contract_id {
                continue;
            }
            if skipped < request.start {
                skipped += 1;
                continue;
            }
            if results.len() >= page_size {
                truncated = true;
                break;
            }

            results.push(json!({
                "key": BASE64_STANDARD.encode(key.suffix()),
                "value": BASE64_STANDARD.encode(&*value.value_bytes())}));
        }

        Ok(json!({
            "truncated": truncated,
            "next": request.start + results.len(),
            "results": results}))
    }
}
