use crate::cli::NodeCli;
use crate::config::NodeConfig;
use crate::wallet_provider::NodeWalletProvider;
use anyhow::{Context, Result, bail};
use neo_protocol::ProtocolSettings;
use neo_rpc::server::RpcServer;
use neo_system::NeoSystem;
use neo_wallets::Wallet as CoreWallet;
use parking_lot::RwLock as ParkingRwLock;
use std::{path::PathBuf, sync::Arc};
use tracing::{info, warn};

#[cfg(feature = "tee")]
pub(crate) fn maybe_enable_tee_runtime(
    cli: &NodeCli,
    node_config: &NodeConfig,
    protocol_settings: &ProtocolSettings,
    system: &Arc<NeoSystem>,
) -> Result<Option<Arc<crate::tee_integration::TeeRuntime>>> {
    // Privacy mode is strictly opt-in and fail-closed: an enabled [privacy]
    // section requires an explicitly requested TEE runtime, and never falls
    // back to running without one.
    let privacy_section = node_config
        .privacy
        .as_ref()
        .filter(|section| section.enabled);

    if !cli.tee && !cli.tee_auto {
        if privacy_section.is_some() {
            bail!(
                "privacy mode is enabled in [privacy] but the TEE runtime was not \
                 requested; pass --tee (or --tee-auto) explicitly"
            );
        }
        crate::tee_integration::clear_active_runtime();
        return Ok(None);
    }

    let privacy = match privacy_section {
        Some(section) => Some(
            crate::tee_integration::PrivacyEncryption::resolve(section)
                .map_err(|err| anyhow::anyhow!(err))
                .context("invalid [privacy] configuration")?,
        ),
        None => None,
    };
    let privacy_on = privacy.is_some();

    let mempool_capacity = node_config
        .mempool
        .as_ref()
        .and_then(|mempool| mempool.max_transactions)
        .unwrap_or_else(|| {
            usize::try_from(protocol_settings.memory_pool_max_transactions)
                .unwrap_or(50_000)
                .max(1)
        });

    let runtime_result = if let Some(mode) = &privacy {
        crate::tee_integration::TeeRuntime::with_privacy(
            cli.tee_data_path.clone(),
            &cli.tee_ordering_policy,
            mempool_capacity,
            mode.clone(),
        )
    } else {
        crate::tee_integration::TeeRuntime::new(
            cli.tee_data_path.clone(),
            &cli.tee_ordering_policy,
            mempool_capacity,
        )
    };

    let runtime = match runtime_result
        .map_err(|e| anyhow::anyhow!(e.to_string()))
        .context("failed to initialize TEE runtime")
    {
        Ok(runtime) => Arc::new(runtime),
        Err(err) if cli.tee_auto => {
            if privacy_on {
                return Err(err.context(
                    "privacy mode is enabled; refusing to continue without the TEE runtime",
                ));
            }
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
            if privacy_on {
                if let Err(shutdown_err) = runtime.shutdown() {
                    warn!(
                        target: "neo::tee",
                        error = %shutdown_err,
                        "failed to shut down privacy-mode runtime after self-check failure"
                    );
                }
                return Err(err.context(
                    "privacy mode is enabled; refusing to continue without passing TEE self-checks",
                ));
            }
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
            if privacy_on {
                if let Err(shutdown_err) = runtime.shutdown() {
                    warn!(
                        target: "neo::tee",
                        error = %shutdown_err,
                        "failed to shut down privacy-mode runtime after attestation failure"
                    );
                }
                return Err(
                    err.context("privacy mode is enabled; refusing to continue without a TEE attestation report")
                );
            }
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

    // The added-callback hands the transaction bytes to the TEE mempool; when
    // the runtime has privacy mode enabled, `add_transaction` seals them into
    // a MempoolCiphertext envelope internally (and zeroizes the plaintext), so
    // the host-side mirror stores only ciphertext plus hash. With privacy off
    // this is byte-for-byte the pre-privacy plaintext mirror.
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
    if !cli.rpc_hardened
        && let Some(server) = rpc_server
    {
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

    if node_config.unlock_wallet.is_active
        && let Some(path) = node_config.unlock_wallet.path.as_ref()
    {
        if node_config.unlock_wallet.password.is_some() {
            warn!(
                target: "neo::tee",
                "unlock_wallet.password is ignored for TEE wallets"
            );
        }
        return PathBuf::from(path);
    }

    cli.tee_data_path.join("wallet")
}
