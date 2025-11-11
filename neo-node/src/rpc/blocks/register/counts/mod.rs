mod header_count;
mod state_height;

use std::sync::Arc;

use neo_rpc::RpcModule;
use neo_runtime::Runtime;
use tokio::sync::RwLock;

pub fn register_count_methods(module: &RpcModule, runtime: Arc<RwLock<Runtime>>) {
    header_count::register_getblockheadercount(module, runtime.clone());
    state_height::register_getstateheight(module, runtime);
}
