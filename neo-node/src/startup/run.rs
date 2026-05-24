//! Main node startup orchestration.

use super::cli::apply_cli_overrides;
use super::config::{
    build_feature_summary, build_state_service_settings, check_storage_access, check_storage_network,
    select_store_provider, validate_node_config,
};
use super::logging::{spawn_metrics_pump, start_health_endpoint_if_enabled};
use super::services::{
    apply_mempool_policy, build_channels_config, maybe_enable_application_logs,
    maybe_enable_dbft_consensus, maybe_enable_hsm_wallet, maybe_enable_oracle_service,
    maybe_enable_state_service_verification, maybe_enable_tokens_tracker,
    maybe_open_wallet, setup_wallet_provider, start_rpc_server_if_enabled,
    validate_contract_management_integrity,
};
#[cfg(feature = "tee")]
use super::services::{maybe_enable_tee_runtime, maybe_enable_tee_wallet};
use super::signal::wait_for_shutdown_signal;
use crate::cli::NodeCli;
use crate::config::NodeConfig;
use crate::rpc_consensus::RpcServerConsensus;
use anyhow::{bail, Context, Result};
use neo_core::neo_system::NeoSystem;
use neo_core::protocol_settings::ProtocolSettings;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::info;
#[cfg(feature = "tee")]
use tracing::error;
use tracing::warn;

pub(crate) async fn run(cli: NodeCli) -> Result<()> {
    let mut node_config = NodeConfig::load(&cli.config)?;
    apply_cli_overrides(&cli, &mut node_config);

    let logging_handles = crate::logging::init_tracing(&node_config.logging, cli.daemon)?;
    let _log_guard = logging_handles.guard;

    let storage_config = node_config.storage_config();
    let storage_path = cli
        .storage
        .as_ref()
        .map(|path| path.to_string_lossy().to_string())
        .or_else(|| node_config.storage_path());
    let backend_name = node_config.storage_backend().map(|name| name.to_string());

    let protocol_settings: ProtocolSettings = node_config.protocol_settings();
    let read_only_storage = node_config.storage.read_only.unwrap_or(false);

    validate_node_config(
        &node_config,
        storage_path.as_deref(),
        backend_name.as_deref(),
        &protocol_settings,
        cli.rpc_hardened,
    )?;

    if cli.check_all {
        check_storage_access(
            backend_name.as_deref(),
            storage_path.as_deref(),
            storage_config.clone(),
        )?;
        info!(target: "neo", "configuration validated; exiting due to --check-all");
        return Ok(());
    }

    if cli.check_storage {
        check_storage_access(
            backend_name.as_deref(),
            storage_path.as_deref(),
            storage_config.clone(),
        )?;
        info!(target: "neo", "storage backend validated; exiting due to --check-storage");
        return Ok(());
    }

    if cli.check_config {
        info!(target: "neo", "configuration validated; exiting due to --check-config");
        return Ok(());
    }

    info!(
        target: "neo",
        seeds = ?protocol_settings.seed_list,
        network_magic = format_args!("0x{:08x}", protocol_settings.network),
        listen_port = node_config.p2p.listen_port,
        storage = storage_path.as_deref().unwrap_or("<none>"),
        backend = backend_name.as_deref().unwrap_or("memory"),
        "using protocol settings"
    );
    info!(
        target: "neo",
        build = %build_feature_summary(),
        "build profile"
    );

    if read_only_storage {
        bail!("read-only storage mode is only supported with --check-* flags; cannot start the node in read-only mode");
    }

    let store_provider = select_store_provider(backend_name.as_deref(), storage_config)?;
    if let (Some(_provider), Some(path)) = (&store_provider, &storage_path) {
        check_storage_network(path, protocol_settings.network, read_only_storage)?;
    }
    if store_provider.is_some() && storage_path.is_none() {
        let backend = backend_name.unwrap_or_else(|| "unknown".to_string());
        bail!(
            "storage backend '{}' requires a data path (--storage or [storage.path])",
            backend
        );
    }

    let state_service_settings = build_state_service_settings(
        &cli,
        &node_config,
        storage_path.as_deref(),
        &protocol_settings,
    )?;
    let dbft_settings = node_config.dbft_settings(&protocol_settings)?;

    let rpc_plugin_config_path = node_config
        .write_rpc_server_plugin_config(&protocol_settings)?
        .map(|path| path.to_string_lossy().to_string());
    if let Some(path) = rpc_plugin_config_path.as_deref() {
        info!(target: "neo", path, "rpc server configuration emitted");
    }

    let system = NeoSystem::new_with_state_service(
        protocol_settings.clone(),
        store_provider.clone(),
        storage_path.clone(),
        state_service_settings.clone(),
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
    .context("failed to initialise NeoSystem")?;

    validate_contract_management_integrity(&system)
        .context("contract management storage integrity check failed")?;

    apply_mempool_policy(&node_config, &system);

    #[cfg(feature = "tee")]
    let tee_runtime =
        maybe_enable_tee_runtime(&cli, &node_config, &protocol_settings, &system)
            .context("failed to initialize TEE runtime")?;

    if let Some(import_path) = cli.import_acc.as_ref() {
        let summary = crate::import_acc::import_acc_file(&system, import_path, storage_path.as_deref())
            .context("failed to import blocks from .acc file")?;
        info!(
            target: "neo",
            declared_start = summary.declared_start,
            declared_count = summary.declared_count,
            imported = summary.imported,
            skipped = summary.skipped,
            final_height = summary.final_height,
            elapsed_secs = summary.elapsed_secs,
            "acc import summary"
        );
        if cli.import_only {
            info!(
                target: "neo",
                "import completed and --import-only is set; shutting down"
            );
            system
                .shutdown()
                .await
                .map_err(|e| anyhow::anyhow!(e.to_string()))
                .context("failed to shut down NeoSystem after import")?;
            return Ok(());
        }
    }

    let _application_logs_service =
        maybe_enable_application_logs(&node_config, &protocol_settings, &system)
            .context("failed to initialise ApplicationLogs")?;

    let _tokens_tracker_service =
        maybe_enable_tokens_tracker(&node_config, &protocol_settings, &system)
            .context("failed to initialise TokensTracker")?;

    let oracle_service = maybe_enable_oracle_service(&node_config, &protocol_settings, &system)
        .context("failed to initialise OracleService")?;

    let channels_config = build_channels_config(&node_config);
    system
        .start_node(channels_config)
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to start P2P subsystem")?;

    let rpc_server = start_rpc_server_if_enabled(
        &node_config,
        system.clone(),
        protocol_settings.network,
        rpc_plugin_config_path.as_deref(),
    )
    .await
    .context("failed to start RPC server")?;

    let needs_wallet_provider = state_service_settings
        .as_ref()
        .map(|settings| settings.auto_verify)
        .unwrap_or(false)
        || oracle_service.is_some()
        || dbft_settings
            .as_ref()
            .map(|settings| settings.auto_start)
            .unwrap_or(false);
    let wallet_provider = setup_wallet_provider(&rpc_server, &system, needs_wallet_provider)
        .context("failed to initialise wallet provider")?;
    maybe_enable_state_service_verification(
        &system,
        &state_service_settings,
        wallet_provider.as_ref(),
    )
    .context("failed to initialise state service verification")?;
    maybe_enable_dbft_consensus(
        &dbft_settings,
        &protocol_settings,
        &system,
        wallet_provider.as_ref(),
    )
    .context("failed to initialise dBFT consensus")?;
    if let Some(server) = rpc_server.as_ref() {
        if dbft_settings
            .as_ref()
            .is_some_and(|settings| settings.network == protocol_settings.network)
        {
            server
                .write()
                .register_handlers(RpcServerConsensus::register_handlers());
        }
    }

    let hsm_wallet_enabled =
        maybe_enable_hsm_wallet(&cli, &node_config, &rpc_server, &system).await?;

    #[cfg(feature = "tee")]
    let tee_wallet_enabled = maybe_enable_tee_wallet(
        &cli,
        &node_config,
        &rpc_server,
        wallet_provider.as_ref(),
        &system,
        tee_runtime.as_ref(),
        hsm_wallet_enabled,
    )
    .context("failed to initialize TEE wallet")?;

    #[cfg(not(feature = "tee"))]
    let tee_wallet_enabled = false;

    if !hsm_wallet_enabled && !tee_wallet_enabled {
        maybe_open_wallet(
            &cli,
            &node_config,
            &rpc_server,
            wallet_provider.as_ref(),
            &system,
        )?;
    }

    let health_state = Arc::new(RwLock::new(crate::health::HealthState::default()));
    start_health_endpoint_if_enabled(&cli, &node_config, health_state.clone()).await;
    let (pump_shutdown_tx, metrics_handle) =
        spawn_metrics_pump(system.clone(), storage_path.clone(), health_state.clone());

    info!(target: "neo", "node running, press Ctrl+C to stop");

    wait_for_shutdown_signal().await;

    let _ = pump_shutdown_tx.send(true);
    let _ = metrics_handle.await;

    if let Some(server) = rpc_server {
        if let Some(mut guard) = server.try_write() {
            guard.stop_rpc_server();
        }
    }

    #[cfg(feature = "tee")]
    if let Some(runtime) = tee_runtime.as_ref() {
        crate::tee_integration::clear_active_runtime();
        if let Err(err) = runtime.shutdown() {
            error!(target: "neo::tee", error = %err, "failed to shut down TEE runtime");
        }
    }

    let store = system.store();
    info!(target: "neo", "flushing storage before shutdown");
    store.flush();
    info!(target: "neo", "storage flush complete");

    match tokio::time::timeout(std::time::Duration::from_secs(30), system.shutdown()).await {
        Ok(Ok(_)) => info!(target: "neo", "system shutdown complete"),
        Ok(Err(e)) => {
            return Err(anyhow::anyhow!(e.to_string())).context("failed to shut down NeoSystem");
        }
        Err(_) => warn!(target: "neo", "system shutdown timed out after 30s, forcing exit"),
    }

    info!(target: "neo", "shutdown complete");
    Ok(())
}
