use std::sync::Arc;

use tracing::info;

use super::config::NodeConfig;

/// Builds the RPC server over the shared node, registers the full
/// provider handler set, and starts it on the configured endpoint.
/// Returns the server handle, kept alive for the node's lifetime.
pub(super) fn start_rpc_server(
    node: &Arc<neo_system::Node>,
    config: &NodeConfig,
    network_magic: u32,
) -> anyhow::Result<Arc<parking_lot::RwLock<neo_rpc::server::RpcServer>>> {
    use neo_rpc::server::{
        RpcServer, RpcServerApplicationLogs, RpcServerBlockchain, RpcServerIndexer, RpcServerNode,
        RpcServerOracle, RpcServerSmartContract, RpcServerState, RpcServerTokensTracker,
        RpcServerUtilities, RpcServerWallet,
    };

    let rpc_config = config.rpc.server_config(network_magic)?;
    let bind_address = rpc_config.bind_address;
    let port = rpc_config.port;
    let mut server = RpcServer::new(Arc::clone(node), rpc_config);
    server.register_handlers(RpcServerBlockchain::register_handlers());
    server.register_handlers(RpcServerNode::register_handlers());
    server.register_handlers(RpcServerState::register_handlers());
    server.register_handlers(RpcServerWallet::register_handlers());
    server.register_handlers(RpcServerUtilities::register_handlers());
    server.register_handlers(RpcServerSmartContract::register_handlers());
    // C#-optional plugin method groups plus the built-in NeoIndexer: register
    // handlers by default so operators get one stable RPC surface. Individual
    // methods report service-not-available when their backing service is not
    // enabled.
    server.register_handlers(RpcServerApplicationLogs::register_handlers());
    server.register_handlers(RpcServerTokensTracker::register_handlers());
    server.register_handlers(RpcServerIndexer::register_handlers());
    server.register_handlers(RpcServerOracle::register_handlers());

    let server = Arc::new(parking_lot::RwLock::new(server));
    let weak = Arc::downgrade(&server);
    server.write().start_rpc_server(weak, None);
    info!(target: "neo", %bind_address, port, "RPC server started");
    Ok(server)
}
