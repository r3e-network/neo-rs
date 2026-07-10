//! Shared StateService lookup and error contracts for state RPC handlers.

use std::sync::Arc;

use neo_state_service::StateStore;
use neo_state_service::mpt_store::MptStore;
use neo_storage::persistence::providers::RuntimeStore;

use super::RpcServerState;
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use crate::server::rpc_server::RpcServer;

/// C# `StateServiceSettings.MaxFindResultItems` default (the plugin
/// caps every `findstates` page at this many results).
pub(super) const MAX_FIND_RESULT_ITEMS: usize = 100;

impl RpcServerState {
    pub(super) fn state_store(
        server: &RpcServer,
    ) -> Result<Arc<StateStore<RuntimeStore>>, RpcException> {
        server.system().state_store().ok_or_else(|| {
            RpcException::from(
                RpcError::internal_server_error().with_data("StateService service not registered"),
            )
        })
    }

    /// Resolves the persisted MPT backend, or reports the same
    /// `UnsupportedState` error the MPT-less build always served.
    pub(super) fn mpt_store(
        server: &RpcServer,
    ) -> Result<Arc<MptStore<RuntimeStore>>, RpcException> {
        let state_store = Self::state_store(server)?;
        state_store.mpt().ok_or_else(Self::proofs_unsupported)
    }

    /// The state-root cache does not persist the MPT trie, so queries
    /// that must walk historical tries cannot be answered.
    pub(super) fn proofs_unsupported() -> RpcException {
        RpcException::from(RpcError::unsupported_state().with_data(
            "the state service in this build records validated state roots only and does not \
             persist the MPT trie required for state/proof queries",
        ))
    }
}
