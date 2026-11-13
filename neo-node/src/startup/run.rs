//! Main node startup orchestration.

use super::cli::apply_cli_overrides;
use super::config::{
    build_feature_summary, build_state_service_settings, check_storage_access,
    check_storage_network, select_store_provider, validate_node_config,
};
use super::logging::{spawn_metrics_pump, start_health_endpoint_if_enabled};
use super::reference_roots::configure_live_reference_roots;
use super::services::{
    apply_mempool_policy, build_channels_config, maybe_enable_application_logs,
    maybe_enable_dbft_consensus, maybe_enable_hsm_wallet, maybe_enable_oracle_service,
    maybe_enable_state_service_verification, maybe_enable_tokens_tracker, maybe_open_wallet,
    setup_wallet_provider, start_rpc_server_if_enabled, validate_contract_management_integrity,
};
#[cfg(feature = "tee")]
use super::services::{maybe_enable_tee_runtime, maybe_enable_tee_wallet};
use super::signal::{parse_live_stop_height, wait_for_live_stop_height, wait_for_shutdown_signal};
use super::tasks::{BackgroundTasks, DEFAULT_BACKGROUND_TASK_SHUTDOWN};
use crate::ai_agent::AiAgentRuntime;
use crate::cli::NodeCli;
use crate::config::{NodeConfig, SignerProviderKind};
use crate::rpc_consensus::RpcServerConsensus;
use crate::signing_provider::{
    ActiveSignerBackend, NODE_SIGNING_TENANT, ORACLE_SIGNING_PRINCIPAL, RPC_SIGNING_PRINCIPAL,
    STATE_SIGNING_PRINCIPAL, initialize_signer_service,
};
use anyhow::{Context, Result, bail};
use neo_protocol::ProtocolSettings;
use neo_rpc::server::RpcSignerRuntime;
use neo_system::{ExecutionMode, NeoSystem};
use neo_wallets::signing::SecurityLevel;
use std::sync::Arc;
use tokio::sync::RwLock;
#[cfg(feature = "tee")]
use tracing::error;
use tracing::info;
use tracing::warn;

pub(crate) async fn run(cli: NodeCli) -> Result<()> {
    // Handle migration if requested
    if cli.migrate_from.is_some() {
        return run_migration(&cli).await;
    }

    let live_stop_height =
        parse_live_stop_height(std::env::var("NEO_LIVE_STOP_HEIGHT").ok().as_deref())
            .map_err(anyhow::Error::msg)?;
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
        bail!(
            "read-only storage mode is only supported with --check-* flags; cannot start the node in read-only mode"
        );
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

    let state_service_settings =
        build_state_service_settings(&node_config, storage_path.as_deref(), &protocol_settings)?;

    // Save auto_verify flag before moving state_service_settings
    let state_service_auto_verify = state_service_settings
        .as_ref()
        .map(|s| s.auto_verify)
        .unwrap_or(false);

    let dbft_settings = node_config.dbft_settings(&protocol_settings)?;

    let rpc_plugin_config_path = node_config
        .write_rpc_server_plugin_config(&protocol_settings)?
        .map(|path| path.to_string_lossy().to_string());
    if let Some(path) = rpc_plugin_config_path.as_deref() {
        info!(target: "neo", path, "rpc server configuration emitted");
    }

    // Genesis must be persisted after ApplicationLogs and TokensTracker are
    // registered. Construction used to persist it immediately, and the .acc
    // import then skipped height 0, so those plugins never saw the genesis block.
    let system = NeoSystem::new_with_state_service_deferring_genesis(
        protocol_settings.clone(),
        store_provider.map(Arc::from),
        storage_path.clone(),
        state_service_settings.clone(),
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
    .context("failed to initialise NeoSystem")?;
    let execution_mode: ExecutionMode = node_config
        .execution
        .mode
        .parse()
        .map_err(anyhow::Error::msg)?;
    system.set_execution_mode(execution_mode);
    info!(
        target: "neo",
        execution_mode = execution_mode.as_str(),
        "transaction execution engine selected"
    );
    configure_live_reference_roots(&system)
        .context("failed to configure live state-root validation")?;

    validate_contract_management_integrity(&system)
        .context("contract management storage integrity check failed")?;

    apply_mempool_policy(&node_config, &system);

    let diagnostics_config = node_config.diagnostics.clone().unwrap_or_default();
    let ai_agent_runtime = Arc::new(
        AiAgentRuntime::new(&diagnostics_config, system.clone())
            .context("failed to initialize AI diagnostics runtime")?,
    );
    ai_agent_runtime
        .configure_mempool(&diagnostics_config)
        .context("failed to configure diagnostics quarantine")?;

    #[cfg(feature = "tee")]
    let tee_runtime = maybe_enable_tee_runtime(&cli, &node_config, &protocol_settings, &system)
        .context("failed to initialize TEE runtime")?;

    // Like neo-cli, imported blocks pass through the plugins, so ApplicationLogs and
    // TokensTracker must be registered before the .acc import starts.
    let _application_logs_service =
        maybe_enable_application_logs(&node_config, &protocol_settings, &system)
            .context("failed to initialise ApplicationLogs")?;

    let _tokens_tracker_service =
        maybe_enable_tokens_tracker(&node_config, &protocol_settings, &system)
            .context("failed to initialise TokensTracker")?;

    system
        .persist_genesis_if_absent()
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to persist genesis block")?;
    if let Some(logs) = _application_logs_service.as_ref() {
        logs.flush();
    }

    let rewrite_vmstate = std::env::var_os("NEO_REWRITE_PERSIST_VMSTATE").is_some();
    let rewrite_gas_exception = std::env::var_os("NEO_REWRITE_GAS_EXCEPTION").is_some();
    if rewrite_vmstate || rewrite_gas_exception {
        if rewrite_vmstate {
            let rewritten = _application_logs_service
                .as_ref()
                .map(|logs| logs.rewrite_native_persist_break_to_halt())
                .transpose()
                .map_err(|err| anyhow::anyhow!(err))
                .context("failed to rewrite native persist vmstate")?
                .unwrap_or(0);
            info!(
                target: "neo",
                rewritten,
                "rewrote OnPersist/PostPersist vmstate BREAK to HALT"
            );
        }
        if rewrite_gas_exception {
            let rewritten = _application_logs_service
                .as_ref()
                .map(|logs| logs.rewrite_fault_exception_messages())
                .transpose()
                .map_err(|err| anyhow::anyhow!(err))
                .context("failed to rewrite fault exception text")?
                .unwrap_or(0);
            info!(
                target: "neo",
                rewritten,
                "rewrote application log fault exceptions to C# text"
            );
        }
        if let Some(logs) = _application_logs_service.as_ref() {
            logs.flush();
        }
        system
            .shutdown()
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
            .context("failed to shut down NeoSystem after application log rewrite")?;
        return Ok(());
    }

    if std::env::var_os("NEO_EXIT_AFTER_GENESIS").is_some() {
        info!(
            target: "neo",
            "genesis persisted and NEO_EXIT_AFTER_GENESIS is set; shutting down"
        );
        system
            .shutdown()
            .await
            .map_err(|e| anyhow::anyhow!(e.to_string()))
            .context("failed to shut down NeoSystem after genesis persist")?;
        return Ok(());
    }

    if let Some(import_path) = cli.import_acc.as_ref() {
        let summary =
            crate::import_acc::import_acc_file(&system, import_path, storage_path.as_deref())
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

    let needs_wallet_provider = state_service_auto_verify
        || oracle_service.is_some()
        || dbft_settings.is_some()
        || node_config
            .signer_service
            .as_ref()
            .is_some_and(|settings| settings.enabled);
    let wallet_provider = setup_wallet_provider(&rpc_server, &system, needs_wallet_provider)
        .context("failed to initialise wallet provider")?;
    if let Some(server) = rpc_server.as_ref()
        && dbft_settings
            .as_ref()
            .is_some_and(|settings| settings.network == protocol_settings.network)
    {
        server
            .write()
            .register_handlers(RpcServerConsensus::register_handlers());
    }

    let hsm_wallet_enabled = maybe_enable_hsm_wallet(
        &cli,
        &node_config,
        &rpc_server,
        wallet_provider.as_ref(),
        &system,
    )
    .await?;

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

    let nep6_wallet_enabled = if !hsm_wallet_enabled && !tee_wallet_enabled {
        maybe_open_wallet(
            &cli,
            &node_config,
            &rpc_server,
            wallet_provider.as_ref(),
            &system,
        )?
    } else {
        false
    };

    let mut active_signer_backend = None;
    #[cfg(feature = "hsm")]
    if hsm_wallet_enabled {
        active_signer_backend = Some(ActiveSignerBackend {
            kind: SignerProviderKind::Hsm,
            security_level: SecurityLevel::Hsm,
        });
    }
    if tee_wallet_enabled {
        active_signer_backend = Some(ActiveSignerBackend {
            kind: SignerProviderKind::HostSealedTee,
            security_level: SecurityLevel::HostSealed,
        });
    } else if nep6_wallet_enabled {
        active_signer_backend = Some(ActiveSignerBackend {
            kind: SignerProviderKind::Nep6,
            security_level: SecurityLevel::Software,
        });
    }

    let signer_runtime = initialize_signer_service(
        node_config.signer_service.as_ref(),
        protocol_settings.network,
        wallet_provider.as_ref(),
        active_signer_backend,
    )
    .context("failed to initialize unified signer service")?;
    if let Some(server) = rpc_server.as_ref() {
        server
            .write()
            .set_signer_runtime(signer_runtime.as_ref().map(|runtime| RpcSignerRuntime {
                service: runtime.service(),
                default_provider: runtime.default_provider().to_string(),
                policy_version: runtime.policy_version().to_string(),
                request_timeout: runtime.request_timeout(),
                key_resolver: runtime.key_resolver(),
                principal: RPC_SIGNING_PRINCIPAL.to_string(),
                tenant: NODE_SIGNING_TENANT.to_string(),
            }))
            .map_err(anyhow::Error::msg)
            .context("failed to install authenticated RPC signer runtime")?;
    }

    let state_signer = if state_service_auto_verify {
        let runtime = signer_runtime
            .as_ref()
            .context("state root verification requires the unified signer service")?;
        runtime.ensure_purpose(&neo_wallets::signing::SignPurpose::StateRoot)?;
        Some(
            runtime
                .trusted_client(
                    STATE_SIGNING_PRINCIPAL,
                    "node-local-state-service",
                    "sign:state-root",
                )
                .map_err(|error| anyhow::anyhow!(error.to_string()))?,
        )
    } else {
        None
    };
    maybe_enable_state_service_verification(
        &system,
        &state_service_settings,
        wallet_provider.as_ref(),
        state_signer,
    )
    .context("failed to initialise state service verification")?;

    if let Some(oracle) = oracle_service.as_ref() {
        let runtime = signer_runtime
            .as_ref()
            .context("OracleService requires the unified signer service")?;
        runtime.ensure_purpose(&neo_wallets::signing::SignPurpose::OracleResponse)?;
        let client = runtime
            .trusted_client(
                ORACLE_SIGNING_PRINCIPAL,
                "node-local-oracle-service",
                "sign:oracle-response",
            )
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        oracle.set_signer_client(client);
    }

    maybe_enable_dbft_consensus(
        &dbft_settings,
        &protocol_settings,
        &system,
        wallet_provider.as_ref(),
    )
    .context("failed to initialise dBFT consensus")?;

    let health_state = Arc::new(RwLock::new(crate::health::HealthState::default()));
    let background_tasks = BackgroundTasks::new();
    start_health_endpoint_if_enabled(&background_tasks, &cli, &node_config, health_state.clone());
    spawn_metrics_pump(
        &background_tasks,
        system.clone(),
        storage_path.clone(),
        health_state.clone(),
    );

    info!(target: "neo", "node running, press Ctrl+C to stop");

    if let Some(stop_height) = live_stop_height {
        tokio::select! {
            _ = wait_for_shutdown_signal() => {}
            _ = wait_for_live_stop_height(system.clone(), stop_height) => {}
        }
    } else {
        wait_for_shutdown_signal().await;
    }

    if !background_tasks
        .shutdown(DEFAULT_BACKGROUND_TASK_SHUTDOWN)
        .await
    {
        warn!(target: "neo", "background task shutdown timed out");
    }

    if let Some(server) = rpc_server
        && let Some(mut guard) = server.try_write()
    {
        guard.stop_rpc_server();
    }

    #[cfg(feature = "tee")]
    if let Some(runtime) = tee_runtime.as_ref() {
        crate::tee_integration::clear_active_runtime();
        if let Err(err) = runtime.shutdown() {
            error!(target: "neo::tee", error = %err, "failed to shut down TEE runtime");
        }
    }

    let store = system.store();
    let state_store = system.state_store().ok().flatten();
    let shutdown_result =
        tokio::time::timeout(std::time::Duration::from_secs(60), system.shutdown()).await;

    // Flush only after actors have stopped (or the shutdown attempt has ended),
    // so no block can be persisted between the final flush and process exit.
    info!(target: "neo", "flushing storage after actor shutdown");
    store.flush();
    // The state service uses a store separate from the chain store; flush it too
    // so computed state roots survive the restart when the WAL is disabled by the
    // high-throughput batch profile.
    if let Some(state_store) = state_store {
        state_store.flush();
    }
    if let Some(logs) = _application_logs_service.as_ref() {
        logs.flush();
    }
    if let Some(tracker) = _tokens_tracker_service.as_ref() {
        tracker.flush();
    }
    info!(target: "neo", "storage flush complete");

    match shutdown_result {
        Ok(Ok(_)) => info!(target: "neo", "system shutdown complete"),
        Ok(Err(e)) => {
            return Err(anyhow::anyhow!(e.to_string())).context("failed to shut down NeoSystem");
        }
        Err(_) => {
            return Err(anyhow::anyhow!("system shutdown timed out after 60s"));
        }
    }

    info!(target: "neo", "shutdown complete");
    Ok(())
}

/// Run migration from neo-cli or neo-go.
async fn run_migration(cli: &NodeCli) -> Result<()> {
    use crate::migration::{MigrationConfig, NodeImplementation, detect_implementation, migrate};

    let migrate_from = cli.migrate_from.as_ref().unwrap();

    info!("Starting migration from: {}", migrate_from.display());

    // Detect or use specified implementation type
    let implementation = if let Some(ref type_str) = cli.migrate_type {
        match type_str.to_lowercase().as_str() {
            "neo-cli" | "neocli" => NodeImplementation::NeoCli,
            "neo-go" | "neogo" => NodeImplementation::NeoGo,
            _ => bail!(
                "Invalid migration type: {}. Use 'neo-cli' or 'neo-go'",
                type_str
            ),
        }
    } else {
        info!("Auto-detecting implementation type...");
        detect_implementation(migrate_from)?
    };

    info!("Detected implementation: {}", implementation.as_str());

    // Determine target storage directory
    let target_dir = cli
        .storage
        .clone()
        .unwrap_or_else(|| std::path::PathBuf::from("./Chain"));

    // Build migration config
    let config = MigrationConfig {
        source_dir: migrate_from.clone(),
        target_dir: target_dir.clone(),
        implementation,
        migrate_storage: !cli.migrate_config_only,
        migrate_config: true,
        verify: true,
    };

    // Perform migration
    info!("Starting migration...");
    info!("  Source: {}", config.source_dir.display());
    info!("  Target: {}", config.target_dir.display());
    info!(
        "  Mode: {}",
        if config.migrate_storage {
            "full (storage + config)"
        } else {
            "config only"
        }
    );

    let result = migrate(config).await?;

    info!("");
    info!(
        "Migration completed for network {} (magic {})",
        result.network, result.protocol_magic
    );
    if cli.migrate_config_only {
        info!("Configuration parsed successfully; no chain data was imported");
        info!("Start neo-rs with a separate configuration to synchronize from genesis");
    } else {
        info!(
            "Imported {} blocks and {} state entries into {}; resume at height {}",
            result.blocks_migrated,
            result.state_entries_migrated,
            target_dir.display(),
            result.starting_height
        );
    }

    Ok(())
}
