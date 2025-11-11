use std::sync::Arc;

use neo_rpc::{RpcError, RpcModule};
use neo_runtime::Runtime;
use tokio::sync::RwLock;

pub fn register_getblockhash(module: &RpcModule, runtime: Arc<RwLock<Runtime>>) {
    module.register("getblockhash", move |params| {
        let runtime = runtime.clone();
        async move {
            let heights: Vec<u64> = params.parse()?;
            let height = heights
                .get(0)
                .copied()
                .ok_or_else(|| RpcError::invalid_params("height is required"))?;
            let guard = runtime.read().await;
            let maybe_hash = guard
                .blockchain()
                .recent_blocks()
                .find(|block| block.index == height)
                .map(|block| block.hash);
            match maybe_hash {
                Some(hash) => Ok(serde_json::json!(hash.to_string())),
                None => Err(RpcError::new(
                    -100,
                    format!("block {height} not found"),
                    None,
                )),
            }
        }
    });
}
