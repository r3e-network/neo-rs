mod best;
mod block;
mod count;

use std::sync::Arc;

use neo_rpc::RpcModule;
use neo_runtime::Runtime;
use tokio::sync::RwLock;

use crate::status::NodeStatus;

pub fn register_hash_methods(
    module: &RpcModule,
    state: Arc<RwLock<NodeStatus>>,
    runtime: Arc<RwLock<Runtime>>,
) {
    count::register_getblockcount(module, state.clone());
    best::register_getbestblockhash(module, runtime.clone());
    block::register_getblockhash(module, runtime);
}
