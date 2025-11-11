use std::sync::Arc;

use neo_rpc::RpcModule;
use tokio::sync::RwLock;

use crate::status::NodeStatus;

pub fn register_getblockcount(module: &RpcModule, state: Arc<RwLock<NodeStatus>>) {
    module.register("getblockcount", move |_params| {
        let state = state.clone();
        async move {
            let snapshot = state.read().await;
            Ok(serde_json::json!(snapshot.height.saturating_add(1)))
        }
    });
}
