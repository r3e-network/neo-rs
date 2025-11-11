use std::sync::Arc;

use neo_base::hash::Hash256;
use neo_rpc::RpcModule;
use neo_runtime::Runtime;
use tokio::sync::RwLock;

pub fn register_getbestblockhash(module: &RpcModule, runtime: Arc<RwLock<Runtime>>) {
    module.register("getbestblockhash", move |_params| {
        let runtime = runtime.clone();
        async move {
            let guard = runtime.read().await;
            match guard.blockchain().last_block() {
                Some(block) => Ok(serde_json::json!(block.hash.to_string())),
                None => Ok(serde_json::json!(Hash256::ZERO.to_string())),
            }
        }
    });
}
