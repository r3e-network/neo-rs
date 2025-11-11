use std::sync::Arc;

use neo_rpc::RpcModule;
use neo_runtime::Runtime;
use tokio::sync::RwLock;

pub fn register_getstateheight(module: &RpcModule, runtime: Arc<RwLock<Runtime>>) {
    module.register("getstateheight", move |_params| {
        let runtime = runtime.clone();
        async move {
            let guard = runtime.read().await;
            let height = guard.blockchain().height();
            Ok(serde_json::json!({
                "blockheight": height,
                "headerheight": height,
            }))
        }
    });
}
