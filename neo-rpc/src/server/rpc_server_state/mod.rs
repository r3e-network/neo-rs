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
//! - `state_queries`: Historical state lookup and `findstates` trie workflows.
//! - `tests`: Module-local tests and regression coverage.

use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::{RpcHandler, RpcServer};
use neo_state_service::StateStore;
use neo_state_service::mpt_store::MptStore;
use neo_state_service::state_store::StateStoreLookup;
use serde_json::{Value, json};
use std::sync::Arc;

mod proof;
mod request;
mod response;
mod state_queries;
use request::{NoParamsRequest, StateRootRequest};
use response::state_root_to_json;

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

    fn get_state_height(server: &RpcServer, params: &[Value]) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "getstateheight")?;
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
