//! State-root and StateService height RPC handlers.

use neo_state_service::state_store::StateStoreLookup;
use serde_json::Value;

use super::RpcServerState;
use super::request::{NoParamsRequest, StateRootRequest};
use super::response::{state_height_to_json, state_root_to_json};
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;

impl RpcServerState {
    pub(super) fn get_state_height(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
        NoParamsRequest::parse(params, "getstateheight")?;
        let state_store = Self::state_store(server)?;
        // The state-root cache records roots once they are validated, so the
        // local and validated indexes coincide in this build. The verification
        // StateStore is only populated when the (currently dormant) state-root
        // verification pipeline runs; fall back to the live MptStore, which is
        // written by the block-apply pipeline, so a running node reports a real
        // height instead of null.
        let index = state_store.current_local_index().or_else(|| {
            Self::mpt_store(server)
                .ok()
                .and_then(|mpt| mpt.current_local_root_index())
        });
        Ok(state_height_to_json(index))
    }

    pub(super) fn get_state_root(
        server: &RpcServer,
        params: &[Value],
    ) -> Result<Value, RpcException> {
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
}
