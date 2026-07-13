//! # neo-node::node::rpc_runtime
//!
//! RPC server runtime wiring and shutdown handling.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `rpc_runtime`: RPC runtime startup and shutdown handle.

use std::sync::Arc;

use neo_storage::persistence::providers::RuntimeStore;
use tracing::info;

use super::config::NodeConfig;
use super::services::NodeServiceHandles;

/// Builds the RPC server over the shared node, registers the full
/// provider handler set, and starts it on the configured endpoint.
/// Returns the server handle, kept alive for the node's lifetime.
pub(super) fn start_rpc_server(
    node: &Arc<neo_system::Node<neo_native_contracts::StandardNativeProvider, RuntimeStore>>,
    services: &NodeServiceHandles<RuntimeStore>,
    config: &NodeConfig,
    network_magic: u32,
    remote_ledger_rpc: Option<&str>,
) -> anyhow::Result<Arc<parking_lot::RwLock<neo_rpc::server::RpcServer>>> {
    use neo_rpc::server::{
        NodeContext, RpcServer, RpcServerApplicationLogs, RpcServerBlockchain, RpcServerIndexer,
        RpcServerNode, RpcServerOracle, RpcServerSmartContract, RpcServerState,
        RpcServerTokensTracker, RpcServerUtilities, RpcServerWallet,
    };

    let rpc_config = config.rpc.server_config(network_magic)?;
    let bind_address = rpc_config.bind_address;
    let port = rpc_config.port;
    let node_ctx: Arc<NodeContext> = Arc::new(NodeContext::from_parts(
        node.settings(),
        node.storage(),
        node.blockchain(),
        node.network(),
        node.mempool(),
        node.header_cache(),
        services.rpc_services(),
        node.native_contract_provider(),
        node.cold_ledger_provider(),
    ));
    let mut server = RpcServer::new(node_ctx, rpc_config);
    if let Some(endpoint) = remote_ledger_rpc {
        server
            .set_remote_ledger_rpc(endpoint)
            .map_err(|err| anyhow::anyhow!(err.error_message()))?;
    }
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
    server
        .write()
        .start_rpc_server(weak)
        .map_err(|err| anyhow::anyhow!("starting RPC server: {err}"))?;
    info!(target: "neo", %bind_address, port, "RPC server started");
    Ok(server)
}
