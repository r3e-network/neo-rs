//! State Service RPC endpoints backed by the canonical `neo-system` state store.
//!
//! Proof and storage-value methods remain unregistered until their wire encoding
//! is implemented against the canonical persistent MPT; no stubbed success
//! responses are exposed.

use neo_system::state_service::{StateRoot, commit_handlers::read_state_root_record};
use serde_json::{Map, Value, json};
use std::sync::Arc;

use super::rpc_error::RpcError;
use super::rpc_exception::RpcException;
use super::rpc_helpers::internal_error;
use super::rpc_server::{RpcHandler, RpcServer};

/// State Service RPC implementation.
pub struct RpcServerState;

impl RpcServerState {
    /// Registers the state-root methods supported by the canonical state store.
    pub fn register_handlers() -> Vec<RpcHandler> {
        super::rpc_handlers![
            "getstateroot" => Self::get_state_root,
            "getstateheight" => Self::get_state_height,
        ]
    }

    /// Returns the persisted state root at one block height.
    pub fn get_state_root(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        Self::ensure_state_service(server)?;
        if params.len() != 1 {
            return Err(RpcException::from(
                RpcError::invalid_params().with_data("getstateroot expects exactly one index"),
            ));
        }
        let index = params[0]
            .as_u64()
            .and_then(|value| u32::try_from(value).ok())
            .ok_or_else(|| {
                RpcException::from(
                    RpcError::invalid_params().with_data("index parameter required (block height)"),
                )
            })?;

        let state_root = read_state_root_record(&server.system().store_cache(), index)
            .map_err(internal_error)?
            .ok_or_else(|| RpcException::from(RpcError::unknown_state_root()))?;
        Ok(Self::state_root_to_json(&state_root))
    }

    /// Returns independent local and quorum-validated state-root heights.
    pub fn get_state_height(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        if !params.is_empty() {
            return Err(RpcException::from(
                RpcError::invalid_params().with_data("getstateheight expects no parameters"),
            ));
        }
        let state_store = Self::ensure_state_service(server)?;
        Ok(json!({
            "localrootindex": state_store.local_root_index(),
            "validatedrootindex": state_store.validated_root_index(),
        }))
    }

    fn ensure_state_service(
        server: &RpcServer,
    ) -> Result<Arc<dyn neo_system::services::StateStoreService>, RpcException> {
        server
            .system()
            .state_store_typed()
            .map_err(internal_error)?
            .ok_or_else(|| {
                RpcException::from(
                    RpcError::method_not_found().with_data("State service not available"),
                )
            })
    }

    fn state_root_to_json(state_root: &StateRoot) -> Value {
        let mut object = Map::new();
        object.insert("version".to_string(), json!(state_root.version));
        object.insert("index".to_string(), json!(state_root.index));
        object.insert(
            "roothash".to_string(),
            json!(state_root.root_hash.to_string()),
        );
        let witnesses = state_root.witness.as_ref().map_or_else(
            || json!([]),
            |witness| {
                use base64::{Engine as _, engine::general_purpose::STANDARD};
                json!([{
                    "invocation": STANDARD.encode(&witness.invocation_script),
                    "verification": STANDARD.encode(&witness.verification_script),
                }])
            },
        );
        object.insert("witnesses".to_string(), witnesses);
        Value::Object(object)
    }
}

#[cfg(test)]
mod tests;
