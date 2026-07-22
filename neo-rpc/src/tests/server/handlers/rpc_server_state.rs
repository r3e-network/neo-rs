use super::*;
use crate::server::rpc_error::RpcError;
use crate::server::rpc_exception::RpcException;
use neo_config::ProtocolSettings;
use neo_state_service::{StateRoot, StateStore};
use neo_storage::persistence::providers::RuntimeStore;
use neo_trie::MptStoreSnapshot;
use serde_json::{Value, json};
use std::collections::HashMap;
use std::sync::Arc;

use crate::server::rpc_server::RpcServer;

fn make_server_with_state() -> (
    Arc<crate::server::NodeContext>,
    Arc<StateStore<RuntimeStore>>,
    RpcServer,
) {
    let state_store = Arc::new(StateStore::<RuntimeStore>::default());
    let system = crate::server::test_support::test_system_with_services(
        ProtocolSettings::default(),
        crate::server::RpcServices::new().with_state_store(Arc::clone(&state_store)),
    );
    let mut server = RpcServer::new(Arc::clone(&system), Default::default());
    server.register_handlers(RpcServerState::register_handlers());
    (system, state_store, server)
}

fn make_server_without_state() -> RpcServer {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let mut server = RpcServer::new(system, Default::default());
    server.register_handlers(RpcServerState::register_handlers());
    server
}

fn call(server: &RpcServer, method: &str, params: &[Value]) -> Result<Value, RpcException> {
    let handler = server
        .handlers_guard()
        .get(&method.to_ascii_lowercase())
        .cloned()
        .unwrap_or_else(|| panic!("handler {method} registered"));
    (handler.callback())(server, params)
}

fn seed_state_root(state_store: &StateStore<RuntimeStore>, index: u32, byte: u8) -> StateRoot {
    let root = StateRoot::new_current(index, neo_primitives::UInt256::from([byte; 32]));
    assert!(state_store.try_add_state_root(root.clone()));
    state_store.commit_validated_state_roots(std::slice::from_ref(&root));
    root
}

/// In-memory MPT snapshot used to build real tries for the
/// `verifyproof` round-trip.
#[derive(Default)]
struct MemoryMptStore {
    data: parking_lot::Mutex<HashMap<Vec<u8>, Vec<u8>>>,
}

impl MptStoreSnapshot for MemoryMptStore {
    fn try_get(&self, key: &[u8]) -> neo_trie::MptResult<Option<Vec<u8>>> {
        Ok(self.data.lock().get(key).cloned())
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> neo_trie::MptResult<()> {
        self.data.lock().insert(key, value);
        Ok(())
    }

    fn delete(&self, key: Vec<u8>) -> neo_trie::MptResult<()> {
        self.data.lock().remove(&key);
        Ok(())
    }
}

#[path = "../rpc_server_state/basics.rs"]
mod basics;
#[path = "../rpc_server_state/find_states.rs"]
mod find_states;
#[path = "../rpc_server_state/mpt_fixture.rs"]
mod mpt_fixture;
#[path = "../rpc_server_state/proof.rs"]
mod proof;
#[path = "../rpc_server_state/state_gates.rs"]
mod state_gates;
#[path = "../rpc_server_state/state_queries.rs"]
mod state_queries;
