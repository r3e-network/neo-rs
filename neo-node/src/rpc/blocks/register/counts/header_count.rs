use std::sync::Arc;

use neo_rpc::RpcModule;
use neo_runtime::Runtime;
use tokio::sync::RwLock;

pub fn register_getblockheadercount(module: &RpcModule, runtime: Arc<RwLock<Runtime>>) {
    module.register("getblockheadercount", move |_params| {
        let runtime = runtime.clone();
        async move {
            let guard = runtime.read().await;
            let height = guard.blockchain().height();
            Ok(serde_json::json!(height.saturating_add(1)))
        }
    });
}
