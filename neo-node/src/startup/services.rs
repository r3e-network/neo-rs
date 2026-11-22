//! Service initialization and plugin setup.

use crate::config::NodeConfig;
use anyhow::{Context, Result};
use neo_p2p::p2p::ChannelsConfig;
use neo_rpc::application_logs::ApplicationLogsService;
use neo_rpc::server::{
    RpcServer, RpcServerApplicationLogs, RpcServerBlockchain, RpcServerNode, RpcServerOracle,
    RpcServerSettings, RpcServerSmartContract, RpcServerState, RpcServerUtilities, RpcServerWallet,
};
use neo_rpc::tokens_tracker::TokensTrackerService;
use neo_system::NeoSystem;
use neo_system::oracle_service::OracleService;
use parking_lot::RwLock as ParkingRwLock;
use serde_json::Value;
use std::sync::Arc;
use tracing::info;

mod features;
#[cfg(feature = "tee")]
mod privacy;
#[cfg(all(test, feature = "tee"))]
mod privacy_tests;
#[cfg(test)]
mod startup_validation_tests;
#[cfg(feature = "tee")]
mod tee;
mod wallet;

pub(crate) use features::{
    maybe_enable_application_logs, maybe_enable_dbft_consensus, maybe_enable_oracle_service,
    maybe_enable_state_service_verification, maybe_enable_tokens_tracker,
    validate_contract_management_integrity,
};
#[cfg(feature = "tee")]
pub(crate) use privacy::install_privacy_rpc_endpoint;
#[cfg(feature = "tee")]
pub(crate) use tee::{maybe_enable_tee_runtime, maybe_enable_tee_wallet};
pub(crate) use wallet::{maybe_enable_hsm_wallet, maybe_open_wallet, setup_wallet_provider};

pub(crate) fn build_channels_config(node_config: &NodeConfig) -> ChannelsConfig {
    node_config.channels_config()
}

pub(crate) fn apply_mempool_policy(node_config: &NodeConfig, system: &Arc<NeoSystem>) {
    let sender_limit = node_config
        .mempool
        .as_ref()
        .and_then(|mempool| mempool.max_transactions_per_sender);

    if let Some(limit) = sender_limit {
        system
            .mempool()
            .lock()
            .set_max_transactions_per_sender(Some(limit));
        info!(
            target: "neo",
            limit,
            "configured mempool max transactions per sender"
        );
    }
}

pub(crate) async fn start_rpc_server_if_enabled(
    node_config: &NodeConfig,
    system: Arc<NeoSystem>,
    network: u32,
    rpc_config_path: Option<&str>,
) -> Result<Option<Arc<ParkingRwLock<RpcServer>>>> {
    if !node_config.rpc.enabled {
        return Ok(None);
    }

    let mut settings_json: Option<Value> = None;
    if let Some(path) = rpc_config_path {
        let raw = tokio::fs::read_to_string(path)
            .await
            .with_context(|| format!("failed to read rpc config at {path}"))?;
        settings_json = Some(serde_json::from_str(&raw).context("invalid rpc config json")?);
    }
    RpcServerSettings::load(settings_json.as_ref())
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to load rpc server settings")?;

    let settings = RpcServerSettings::current()
        .server_for_network(network)
        .unwrap_or_default();

    let has_application_logs = system
        .get_service::<ApplicationLogsService>()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .is_some();
    let has_tokens_tracker = system
        .get_service::<TokensTrackerService>()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .is_some();
    let has_oracle_service = system
        .get_service::<OracleService>()
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .is_some();
    let has_state_service = system.state_store().ok().flatten().is_some();

    let mut server = RpcServer::new(system, settings);
    server.register_handlers(RpcServerNode::register_handlers());
    server.register_handlers(RpcServerBlockchain::register_handlers());
    server.register_handlers(RpcServerUtilities::register_handlers());
    server.register_handlers(RpcServerSmartContract::register_handlers());
    server.register_handlers(RpcServerWallet::register_handlers());
    if has_application_logs {
        server.register_handlers(RpcServerApplicationLogs::register_handlers());
    }
    if has_state_service {
        server.register_handlers(RpcServerState::register_handlers());
    }
    if has_tokens_tracker {
        server.register_handlers(neo_rpc::server::RpcServerTokensTracker::register_handlers());
    }
    if has_oracle_service {
        server.register_handlers(RpcServerOracle::register_handlers());
    }

    // Privacy mode (strictly opt-in): register the encrypted submission
    // endpoint only when the active TEE runtime has privacy enabled. Without
    // it the method is absent from the RPC surface entirely.
    #[cfg(feature = "tee")]
    install_privacy_rpc_endpoint(
        &mut server,
        crate::tee_integration::active_runtime().as_ref(),
    )?;

    let tls_config = match neo_rpc::server::build_tls_config_from_settings(server.settings()).await
    {
        Ok(config) => config,
        Err(err) => {
            tracing::error!("RPC TLS configuration error: {err}");
            None
        }
    };

    let handle = Arc::new(ParkingRwLock::new(server));
    neo_rpc::server::register_server(network, Arc::clone(&handle));
    handle
        .write()
        .start_rpc_server(Arc::downgrade(&handle), tls_config);

    Ok(Some(handle))
}
