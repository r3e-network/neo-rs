use crate::config::{
    DbftSettings, NodeConfig, resolve_application_logs_store_path,
    resolve_tokens_tracker_store_path,
};
use crate::consensus::{DbftConsensusController, ensure_wallet_can_validate};
use crate::wallet_provider::NodeWalletProvider;
use anyhow::{Context, Result, bail};
use neo_native_contracts::native::ContractManagement;
use neo_protocol::ProtocolSettings;
use neo_rpc::application_logs::ApplicationLogsService;
use neo_rpc::tokens_tracker::{TokensTracker, TokensTrackerService};
use neo_state_service::StateServiceSettings;
use neo_system::i_event_handlers::{CommittedHandler, CommittingHandler, WalletChangedHandler};
use neo_system::oracle_service::OracleService;
use neo_system::{NeoSystem, StateServiceVerification};
use neo_wallets::WalletProvider;
use neo_wallets::signing::TrustedSignerClient;
use std::{fs, sync::Arc};
use tracing::{info, warn};

pub(crate) fn validate_contract_management_integrity(system: &Arc<NeoSystem>) -> Result<()> {
    let store_cache = system.store_cache();
    ContractManagement::validate_snapshot_integrity(store_cache.data_cache())
        .map_err(|error| anyhow::anyhow!(error.to_string()))?;
    let non_native_contracts = ContractManagement::list_contracts(store_cache.data_cache())
        .map_err(|error| anyhow::anyhow!(error.to_string()))?
        .len();
    info!(
        target: "neo",
        non_native_contracts, "ContractManagement snapshot integrity check passed"
    );
    Ok(())
}
pub(crate) fn maybe_enable_state_service_verification(
    system: &Arc<NeoSystem>,
    state_service_settings: &Option<StateServiceSettings>,
    wallet_provider: Option<&Arc<NodeWalletProvider>>,
    signer: Option<TrustedSignerClient>,
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
        bail!("state root verification requires a wallet provider");
    }
    let signer = signer.context("state root verification requires the unified signer service")?;

    system
        .register_wallet_changed_handler(Arc::new(StateServiceVerification::new(
            system.store(),
            system.settings().clone(),
            signer,
        )) as Arc<dyn WalletChangedHandler + Send + Sync>)
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

    if settings.ignore_recovery_logs {
        bail!("dBFT requires recovery logs; IgnoreRecoveryLogs is unsafe");
    }
    if settings.recovery_logs.trim().is_empty() {
        bail!("dBFT requires a non-empty durable recovery path");
    }
    if settings.network != protocol_settings.network {
        bail!(
            "dBFT network mismatch: configured 0x{:08x}, expected 0x{:08x}",
            settings.network,
            protocol_settings.network
        );
    }

    let provider = wallet_provider.context(
        "dBFT validator mode requires a wallet provider; refusing to start a node that cannot produce blocks",
    )?;
    let wallet = provider.get_wallet().context(
        "dBFT validator mode requires a loaded wallet; refusing to start a node that cannot produce blocks",
    )?;
    ensure_wallet_can_validate(system, &wallet).map_err(anyhow::Error::msg)?;

    let controller = Arc::new(DbftConsensusController::new(
        system.clone(),
        settings.clone(),
    ));
    system
        .add_service::<DbftConsensusController, _>(Arc::clone(&controller))
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to register dBFT consensus service")?;
    system
        .register_wallet_changed_handler(controller as Arc<dyn WalletChangedHandler + Send + Sync>)
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
