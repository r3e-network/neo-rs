use super::*;
use neo_config::ProtocolSettings;
use neo_crypto::mpt_trie::MptStoreSnapshot;
use neo_state_service::StateRoot;
use serde_json::json;
use std::collections::HashMap;

use crate::server::rpc_server::RpcServer;

fn make_server_with_state() -> (Arc<neo_system::Node>, Arc<StateStore>, RpcServer) {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let state_store = Arc::new(StateStore::new());
    system.register_service(Arc::clone(&state_store));
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

fn seed_state_root(state_store: &StateStore, index: u32, byte: u8) -> StateRoot {
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
    fn try_get(&self, key: &[u8]) -> neo_crypto::mpt_trie::MptResult<Option<Vec<u8>>> {
        Ok(self.data.lock().get(key).cloned())
    }

    fn put(&self, key: Vec<u8>, value: Vec<u8>) -> neo_crypto::mpt_trie::MptResult<()> {
        self.data.lock().insert(key, value);
        Ok(())
    }

    fn delete(&self, key: Vec<u8>) -> neo_crypto::mpt_trie::MptResult<()> {
        self.data.lock().remove(&key);
        Ok(())
    }
}

mod basics;
mod find_states;
mod mpt_fixture;
mod proof;
mod state_gates;
mod state_queries;
