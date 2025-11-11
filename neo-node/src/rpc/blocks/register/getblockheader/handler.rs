use std::sync::Arc;

use neo_rpc::{RpcError, RpcModule};
use neo_runtime::Runtime;
use serde_json::json;
use tokio::sync::RwLock;

use crate::rpc::blocks::utils::{block_header_to_json, lookup_block, normalize_params};

pub fn register_getblockheader(module: &RpcModule, runtime: Arc<RwLock<Runtime>>) {
    module.register("getblockheader", move |params| {
        let runtime = runtime.clone();
        async move {
            let args = normalize_params(&params)?;
            if args.is_empty() {
                return Err(RpcError::invalid_params(
                    "expected block hash or height argument",
                ));
            }
            let lookup = &args[0];
            let verbose = args
                .get(1)
                .and_then(|value| value.as_bool())
                .unwrap_or(true);

            let guard = runtime.read().await;
            let summary = lookup_block(&guard, lookup)?;
            match summary {
                Some(block) => {
                    if verbose {
                        Ok(block_header_to_json(block))
                    } else {
                        Ok(json!(hex::encode(block.hash.as_slice())))
                    }
                }
                None => Err(RpcError::new(
                    -100,
                    "requested block not available in in-memory snapshot",
                    None,
                )),
            }
        }
    });
}
