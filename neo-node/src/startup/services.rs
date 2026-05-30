//! Service initialization and plugin setup.

use crate::cli::NodeCli;
use crate::config::{
    resolve_application_logs_store_path, resolve_tokens_tracker_store_path, DbftSettings,
    NodeConfig,
};
use crate::consensus::DbftConsensusController;
use crate::wallet_provider::NodeWalletProvider;
use anyhow::{bail, Context, Result};
use neo_application_logs::ApplicationLogsService;
use neo_core::{
    i_event_handlers::{CommittedHandler, CommittingHandler, WalletChangedHandler},
    neo_system::NeoSystem,
    network::p2p::channels_config::ChannelsConfig,
    oracle_service::OracleService,
    protocol_settings::ProtocolSettings,
    smart_contract::native::ContractManagement,
    state_service::{
        state_store::StateServiceSettings,
        verification::StateServiceVerification,
    },
    tokens_tracker::{TokensTracker, TokensTrackerService},
    wallets::{WalletProvider, Nep6Wallet, Wallet as CoreWallet},
};
use neo_rpc::server::{
    RpcServer, RpcServerApplicationLogs, RpcServerBlockchain, RpcServerNode, RpcServerOracle,
    RpcServerSettings, RpcServerSmartContract, RpcServerState, RpcServerUtilities, RpcServerWallet,
};
use parking_lot::RwLock as ParkingRwLock;
use serde_json::Value;
use std::{fs, path::PathBuf, sync::Arc};
use tracing::{info, warn};

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

pub(crate) fn validate_contract_management_integrity(system: &Arc<NeoSystem>) -> Result<()> {
    let store_cache = system.store_cache();
    ContractManagement::validate_snapshot_integrity(store_cache.data_cache())
        .map_err(|e| anyhow::anyhow!(e.to_string()))?;

    let non_native_contracts = ContractManagement::list_contracts(store_cache.data_cache())
        .map_err(|e| anyhow::anyhow!(e.to_string()))?
        .len();
    info!(
        target: "neo",
        non_native_contracts,
        "ContractManagement snapshot integrity check passed"
    );
    Ok(())
}

#[cfg(feature = "tee")]
pub(crate) fn maybe_enable_tee_runtime(
    cli: &NodeCli,
    node_config: &NodeConfig,
    protocol_settings: &ProtocolSettings,
    system: &Arc<NeoSystem>,
) -> Result<Option<Arc<crate::tee_integration::TeeRuntime>>> {
    if !cli.tee && !cli.tee_auto {
        crate::tee_integration::clear_active_runtime();
        return Ok(None);
    }

    let mempool_capacity = node_config
        .mempool
        .as_ref()
        .and_then(|mempool| mempool.max_transactions)
        .unwrap_or_else(|| {
            usize::try_from(protocol_settings.memory_pool_max_transactions)
                .unwrap_or(50_000)
                .max(1)
        });

    let runtime = match crate::tee_integration::TeeRuntime::new(
        cli.tee_data_path.clone(),
        &cli.tee_ordering_policy,
        mempool_capacity,
    )
    .map_err(|e| anyhow::anyhow!(e.to_string()))
    .context("failed to initialize TEE runtime")
    {
        Ok(runtime) => Arc::new(runtime),
        Err(err) if cli.tee_auto => {
            crate::tee_integration::clear_active_runtime();
            warn!(
                target: "neo::tee",
                error = %err,
                "TEE auto mode: runtime initialization failed; continuing without TEE"
            );
            return Ok(None);
        }
        Err(err) => return Err(err),
    };

    if let Err(err) = runtime
        .run_startup_self_checks()
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("TEE startup self-check failed")
    {
        if cli.tee_auto {
            if let Err(shutdown_err) = runtime.shutdown() {
                warn!(
                    target: "neo::tee",
                    error = %shutdown_err,
                    "TEE auto mode: failed to shut down runtime after startup self-check failure"
                );
            }
            warn!(
                target: "neo::tee",
                error = %err,
                "TEE auto mode: startup self-checks failed; continuing without TEE"
            );
            crate::tee_integration::clear_active_runtime();
            return Ok(None);
        }
        return Err(err);
    }
    info!(target: "neo::tee", "TEE startup self-checks passed");

    let attestation_report = match runtime
        .generate_attestation()
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to generate TEE attestation report")
    {
        Ok(report) => report,
        Err(err) if cli.tee_auto => {
            if let Err(shutdown_err) = runtime.shutdown() {
                warn!(
                    target: "neo::tee",
                    error = %shutdown_err,
                    "TEE auto mode: failed to shut down runtime after attestation failure"
                );
            }
            warn!(
                target: "neo::tee",
                error = %err,
                "TEE auto mode: attestation generation failed; continuing without TEE"
            );
            crate::tee_integration::clear_active_runtime();
            return Ok(None);
        }
        Err(err) => return Err(err),
    };
    info!(
        target: "neo::tee",
        bytes = attestation_report.len(),
        "TEE attestation report generated"
    );

    install_tee_mempool_bridge(system, Arc::clone(&runtime));
    info!(
        target: "neo::tee",
        "TEE mempool bridge installed for canonical mempool events"
    );
    crate::tee_integration::register_active_runtime(&runtime);

    if cli.tee_auto {
        info!(
            target: "neo::tee",
            "TEE auto mode: runtime initialized successfully"
        );
    }

    Ok(Some(runtime))
}

#[cfg(feature = "tee")]
fn install_tee_mempool_bridge(
    system: &Arc<NeoSystem>,
    runtime: Arc<crate::tee_integration::TeeRuntime>,
) {
    let mempool = system.mempool();
    let mut guard = mempool.lock();

    let existing_added = guard.transaction_added.take();
    let runtime_for_added = Arc::clone(&runtime);
    guard.transaction_added = Some(Box::new(move |pool, tx| {
        if let Some(handler) = &existing_added {
            handler(pool, tx);
        }

        let mut tx_hash = [0u8; 32];
        tx_hash.copy_from_slice(tx.hash().as_bytes().as_ref());

        let mut sender = [0u8; 20];
        if let Some(script_hash) = tx.sender() {
            sender.copy_from_slice(script_hash.as_bytes().as_ref());
        }

        if let Err(err) = runtime_for_added.mempool.add_transaction(
            tx_hash,
            tx.to_bytes(),
            tx.network_fee(),
            tx.system_fee(),
            sender,
        ) {
            let duplicate = matches!(
                &err,
                neo_tee::TeeError::Other(message) if message.contains("already in pool")
            );
            if !duplicate {
                warn!(
                    target: "neo::tee",
                    tx_hash = %tx.hash(),
                    error = %err,
                    "failed to mirror transaction into TEE mempool"
                );
            }
        }
    }));

    let existing_removed = guard.transaction_removed.take();
    let runtime_for_removed = Arc::clone(&runtime);
    guard.transaction_removed = Some(Box::new(move |pool, args| {
        if let Some(handler) = &existing_removed {
            handler(pool, args);
        }

        for tx in &args.transactions {
            let mut tx_hash = [0u8; 32];
            tx_hash.copy_from_slice(tx.hash().as_bytes().as_ref());
            runtime_for_removed.mempool.remove_transaction(&tx_hash);
        }
    }));
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
            .with_context(|| format!("failed to read rpc config at {}", path))?;
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

    let tls_config = match neo_rpc::server::build_tls_config_from_settings(server.settings()).await
    {
        Ok(config) => config,
        Err(err) => {
            tracing::error!("RPC TLS configuration error: {}", err);
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

pub(crate) fn setup_wallet_provider(
    rpc_server: &Option<Arc<ParkingRwLock<RpcServer>>>,
    system: &Arc<NeoSystem>,
    enable: bool,
) -> Result<Option<Arc<NodeWalletProvider>>> {
    if !enable {
        return Ok(None);
    }

    let provider = Arc::new(NodeWalletProvider::new());
    let provider_trait: Arc<dyn WalletProvider + Send + Sync> = provider.clone();
    system
        .attach_wallet_provider(provider_trait)
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to attach wallet provider")?;

    if let Some(server) = rpc_server {
        let callback_provider = Arc::clone(&provider);
        server
            .write()
            .set_wallet_change_callback(Some(Arc::new(move |wallet| {
                callback_provider.set_wallet(wallet);
            })));
        info!(target: "neo", "wallet provider enabled");
    } else {
        info!(
            target: "neo",
            "wallet provider enabled without RPC callback; wallet updates rely on local wallet loading"
        );
    }

    Ok(Some(provider))
}

pub(crate) fn maybe_enable_state_service_verification(
    system: &Arc<NeoSystem>,
    state_service_settings: &Option<StateServiceSettings>,
    wallet_provider: Option<&Arc<NodeWalletProvider>>,
) -> Result<()> {
    let Some(settings) = state_service_settings else {
        return Ok(());
    };

    if !settings.auto_verify {
        info!(
            target: "neo",
            "state service verification disabled (auto_verify=false)"
        );
        return Ok(());
    }

    if wallet_provider.is_none() {
        warn!(
            target: "neo",
            "state root verification requires wallet provider; skipping"
        );
        return Ok(());
    }

    system
        .register_wallet_changed_handler(Arc::new(StateServiceVerification::new(system.clone()))
            as Arc<dyn WalletChangedHandler + Send + Sync>)
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to register state verification handler")?;

    info!(target: "neo", "state service verification enabled");
    Ok(())
}

pub(crate) fn maybe_enable_dbft_consensus(
    dbft_settings: &Option<DbftSettings>,
    protocol_settings: &ProtocolSettings,
    system: &Arc<NeoSystem>,
    wallet_provider: Option<&Arc<NodeWalletProvider>>,
) -> Result<()> {
    let Some(settings) = dbft_settings else {
        return Ok(());
    };

    if settings.network != protocol_settings.network {
        warn!(
            target: "neo",
            configured = format_args!("0x{:08x}", settings.network),
            expected = format_args!("0x{:08x}", protocol_settings.network),
            "dBFT network mismatch; skipping"
        );
        return Ok(());
    }

    if settings.auto_start && wallet_provider.is_none() {
        warn!(
            target: "neo",
            "dBFT auto_start requires wallet provider; skipping"
        );
        return Ok(());
    }

    system
        .register_wallet_changed_handler({
            let controller = Arc::new(DbftConsensusController::new(
                system.clone(),
                settings.clone(),
            ));
            system
                .add_service::<DbftConsensusController, _>(Arc::clone(&controller))
                .map_err(|e| anyhow::anyhow!(e.to_string()))
                .context("failed to register dBFT consensus service")?;
            controller as Arc<dyn WalletChangedHandler + Send + Sync>
        })
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to register dBFT wallet handler")?;

    info!(
        target: "neo",
        auto_start = settings.auto_start,
        recovery_logs = %settings.recovery_logs,
        "dBFT consensus enabled"
    );
    if !settings.auto_start {
        info!(
            target: "neo",
            "dBFT auto_start disabled; consensus will not start automatically"
        );
    }

    Ok(())
}

pub(crate) fn maybe_enable_application_logs(
    node_config: &NodeConfig,
    protocol_settings: &ProtocolSettings,
    system: &Arc<NeoSystem>,
) -> Result<Option<Arc<ApplicationLogsService>>> {
    let Some(settings) = node_config.application_logs_settings(protocol_settings)? else {
        return Ok(None);
    };

    if settings.network != protocol_settings.network {
        warn!(
            target: "neo",
            configured = format_args!("0x{:08x}", settings.network),
            expected = format_args!("0x{:08x}", protocol_settings.network),
            "ApplicationLogs network mismatch; skipping"
        );
        return Ok(None);
    }

    let store_path = resolve_application_logs_store_path(&settings);
    if let Some(parent) = store_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create ApplicationLogs directory {}",
                parent.display()
            )
        })?;
    }
    let path = store_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("ApplicationLogs path is not valid UTF-8"))?;
    let store = system
        .store_provider()
        .get_store(path)
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to open ApplicationLogs store")?;

    let service = Arc::new(ApplicationLogsService::new(settings.clone(), store));
    system
        .add_service::<ApplicationLogsService, _>(Arc::clone(&service))
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to register ApplicationLogs service")?;
    system
        .register_committing_handler(
            Arc::clone(&service) as Arc<dyn CommittingHandler + Send + Sync>
        )
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to register ApplicationLogs committing handler")?;
    system
        .register_committed_handler(Arc::clone(&service) as Arc<dyn CommittedHandler + Send + Sync>)
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to register ApplicationLogs committed handler")?;

    info!(
        target: "neo",
        path = %store_path.display(),
        debug = settings.debug,
        "ApplicationLogs enabled"
    );

    Ok(Some(service))
}

pub(crate) fn maybe_enable_tokens_tracker(
    node_config: &NodeConfig,
    protocol_settings: &ProtocolSettings,
    system: &Arc<NeoSystem>,
) -> Result<Option<Arc<TokensTrackerService>>> {
    let Some(settings) = node_config.tokens_tracker_settings(protocol_settings)? else {
        return Ok(None);
    };

    if settings.network != protocol_settings.network {
        warn!(
            target: "neo",
            configured = format_args!("0x{:08x}", settings.network),
            expected = format_args!("0x{:08x}", protocol_settings.network),
            "TokensTracker network mismatch; skipping"
        );
        return Ok(None);
    }

    let store_path = resolve_tokens_tracker_store_path(&settings);
    if let Some(parent) = store_path.parent() {
        fs::create_dir_all(parent).with_context(|| {
            format!(
                "failed to create TokensTracker directory {}",
                parent.display()
            )
        })?;
    }
    let path = store_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("TokensTracker path is not valid UTF-8"))?;
    let store = system
        .store_provider()
        .get_store(path)
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to open TokensTracker store")?;

    let tracker = Arc::new(TokensTracker::new(
        settings.clone(),
        store.clone(),
        system.clone(),
    ));
    system
        .register_committing_handler(
            Arc::clone(&tracker) as Arc<dyn CommittingHandler + Send + Sync>
        )
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to register TokensTracker committing handler")?;
    system
        .register_committed_handler(Arc::clone(&tracker) as Arc<dyn CommittedHandler + Send + Sync>)
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to register TokensTracker committed handler")?;

    let service = Arc::new(TokensTrackerService::new(settings.clone(), store));
    system
        .add_service::<TokensTrackerService, _>(Arc::clone(&service))
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to register TokensTracker service")?;

    info!(
        target: "neo",
        path = %store_path.display(),
        trackers = ?settings.enabled_trackers,
        history = settings.track_history,
        "TokensTracker enabled"
    );

    Ok(Some(service))
}

pub(crate) fn maybe_enable_oracle_service(
    node_config: &NodeConfig,
    protocol_settings: &ProtocolSettings,
    system: &Arc<NeoSystem>,
) -> Result<Option<Arc<OracleService>>> {
    if !node_config.rpc.enabled {
        warn!(
            target: "neo",
            "OracleService requires RPC server; skipping"
        );
        return Ok(None);
    }

    let Some(settings) = node_config.oracle_service_settings(protocol_settings)? else {
        return Ok(None);
    };

    if settings.network != protocol_settings.network {
        warn!(
            target: "neo",
            configured = format_args!("0x{:08x}", settings.network),
            expected = format_args!("0x{:08x}", protocol_settings.network),
            "OracleService network mismatch; skipping"
        );
        return Ok(None);
    }

    let service = Arc::new(
        OracleService::new(settings.clone(), system.clone())
            .map_err(|e| anyhow::anyhow!(e.to_string()))?,
    );
    service.set_self_ref();

    system
        .add_service::<OracleService, _>(Arc::clone(&service))
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to register OracleService")?;
    system
        .register_committing_handler(
            Arc::clone(&service) as Arc<dyn CommittingHandler + Send + Sync>
        )
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to register OracleService committing handler")?;
    system
        .register_wallet_changed_handler(
            Arc::clone(&service) as Arc<dyn WalletChangedHandler + Send + Sync>
        )
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to register OracleService wallet handler")?;

    info!(
        target: "neo",
        auto_start = settings.auto_start,
        nodes = settings.nodes.len(),
        "OracleService enabled"
    );

    Ok(Some(service))
}

pub(crate) async fn maybe_enable_hsm_wallet(
    cli: &NodeCli,
    node_config: &NodeConfig,
    rpc_server: &Option<Arc<ParkingRwLock<RpcServer>>>,
    system: &Arc<NeoSystem>,
) -> Result<bool> {
    #[cfg(feature = "hsm")]
    {
        if !cli.hsm {
            return Ok(false);
        }

        let Some(server) = rpc_server else {
            warn!(target: "neo", "HSM requested but RPC is disabled; skipping HSM wallet");
            return Ok(false);
        };

        if resolve_wallet_config(cli, node_config)?.is_some() {
            warn!(
                target: "neo",
                "HSM requested; ignoring NEP-6 wallet configuration to avoid conflicting signers"
            );
        }

        let runtime =
            crate::hsm_integration::initialize_hsm(cli, system.settings().address_version)
                .await
                .context("failed to initialize HSM")?;
        crate::hsm_integration::print_hsm_status(&runtime);
        let wallet =
            crate::hsm_wallet::HsmWallet::from_runtime(runtime, Arc::new(system.settings().clone()))
                .await
                .context("failed to build HSM wallet")?;
        server.write().set_wallet(Some(Arc::new(wallet)));
        return Ok(true);
    }

    #[cfg(not(feature = "hsm"))]
    {
        let _ = (cli, node_config, rpc_server, system);
        Ok(false)
    }
}

#[cfg(feature = "tee")]
pub(crate) fn maybe_enable_tee_wallet(
    cli: &NodeCli,
    node_config: &NodeConfig,
    rpc_server: &Option<Arc<ParkingRwLock<RpcServer>>>,
    wallet_provider: Option<&Arc<NodeWalletProvider>>,
    system: &Arc<NeoSystem>,
    tee_runtime: Option<&Arc<crate::tee_integration::TeeRuntime>>,
    hsm_wallet_enabled: bool,
) -> Result<bool> {
    if hsm_wallet_enabled {
        if tee_runtime.is_some() {
            warn!(
                target: "neo::tee",
                "TEE runtime is enabled but HSM wallet is active; skipping TEE wallet adapter"
            );
        }
        return Ok(false);
    }

    let Some(runtime) = tee_runtime else {
        return Ok(false);
    };

    if rpc_server.is_none() && wallet_provider.is_none() {
        warn!(
            target: "neo::tee",
            "TEE wallet requested but neither RPC nor wallet provider is active; skipping"
        );
        return Ok(false);
    }

    let wallet_path = resolve_tee_wallet_path(cli, node_config);
    let wallet = crate::tee_wallet::TeeWalletAdapter::from_runtime(
        Arc::clone(runtime),
        Arc::new(system.settings().clone()),
        &wallet_path,
    )
    .context("failed to build TEE wallet adapter")?;

    let wallet_arc: Arc<dyn CoreWallet> = Arc::new(wallet);
    if let Some(provider) = wallet_provider {
        provider.set_wallet(Some(wallet_arc.clone()));
    }
    if let Some(server) = rpc_server {
        server.write().set_wallet(Some(wallet_arc));
    }

    info!(
        target: "neo::tee",
        path = %wallet_path.display(),
        "TEE wallet adapter enabled for signing"
    );
    Ok(true)
}

#[cfg(feature = "tee")]
fn resolve_tee_wallet_path(cli: &NodeCli, node_config: &NodeConfig) -> PathBuf {
    if let Some(path) = cli.wallet.clone() {
        if cli.wallet_password.is_some() || node_config.unlock_wallet.password.is_some() {
            warn!(
                target: "neo::tee",
                "wallet password settings are ignored for TEE wallets"
            );
        }
        return path;
    }

    if node_config.unlock_wallet.is_active {
        if let Some(path) = node_config.unlock_wallet.path.as_ref() {
            if node_config.unlock_wallet.password.is_some() {
                warn!(
                    target: "neo::tee",
                    "unlock_wallet.password is ignored for TEE wallets"
                );
            }
            return PathBuf::from(path);
        }
    }

    cli.tee_data_path.join("wallet")
}

pub(crate) fn resolve_wallet_config(
    cli: &NodeCli,
    node_config: &NodeConfig,
) -> Result<Option<(PathBuf, String)>> {
    if let Some(path) = cli.wallet.clone() {
        let password = cli
            .wallet_password
            .clone()
            .or_else(|| node_config.unlock_wallet.password.clone())
            .ok_or_else(|| anyhow::anyhow!("wallet password required (--wallet-password)"))?;
        return Ok(Some((path, password)));
    }

    if node_config.unlock_wallet.is_active {
        let path = node_config
            .unlock_wallet
            .path
            .clone()
            .ok_or_else(|| anyhow::anyhow!("unlock_wallet.path must be set when enabled"))?;
        let password =
            node_config.unlock_wallet.password.clone().ok_or_else(|| {
                anyhow::anyhow!("unlock_wallet.password must be set when enabled")
            })?;
        return Ok(Some((PathBuf::from(path), password)));
    }

    Ok(None)
}

pub(crate) fn maybe_open_wallet(
    cli: &NodeCli,
    node_config: &NodeConfig,
    rpc_server: &Option<Arc<ParkingRwLock<RpcServer>>>,
    wallet_provider: Option<&Arc<NodeWalletProvider>>,
    system: &Arc<NeoSystem>,
) -> Result<()> {
    let Some((wallet_path, password)) = resolve_wallet_config(cli, node_config)? else {
        return Ok(());
    };

    if rpc_server.is_none() && wallet_provider.is_none() {
        warn!(
            target: "neo",
            path = %wallet_path.display(),
            "wallet configured but neither RPC nor wallet provider is active; skipping wallet load"
        );
        return Ok(());
    }

    if !wallet_path.exists() {
        bail!("wallet file not found: {}", wallet_path.display());
    }

    let settings = Arc::new(system.settings().clone());
    let wallet_path_str = wallet_path
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("wallet path is not valid UTF-8"))?;
    let wallet = Nep6Wallet::from_file(wallet_path_str, &password, settings)
        .map_err(|err| anyhow::anyhow!(err.to_string()))
        .context("failed to open wallet")?;
    let wallet_arc: Arc<dyn CoreWallet> = Arc::new(wallet);
    if let Some(provider) = wallet_provider {
        provider.set_wallet(Some(wallet_arc.clone()));
    }
    if let Some(server) = rpc_server {
        server.write().set_wallet(Some(wallet_arc.clone()));
    }

    if rpc_server.is_some() {
        info!(
            target: "neo",
            path = %wallet_path.display(),
            "wallet opened for RPC signing"
        );
    } else {
        info!(
            target: "neo",
            path = %wallet_path.display(),
            "wallet opened for local services"
        );
    }
    Ok(())
}
