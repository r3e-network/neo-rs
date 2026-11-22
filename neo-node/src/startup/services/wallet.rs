use crate::cli::NodeCli;
use crate::config::NodeConfig;
use crate::wallet_provider::NodeWalletProvider;
use anyhow::{Context, Result, bail};
use neo_rpc::server::RpcServer;
use neo_system::NeoSystem;
use neo_wallets::{Nep6Wallet, Wallet as CoreWallet, WalletProvider};
use parking_lot::RwLock as ParkingRwLock;
use std::{path::PathBuf, sync::Arc};
use tracing::{info, warn};

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
pub(crate) async fn maybe_enable_hsm_wallet(
    cli: &NodeCli,
    node_config: &NodeConfig,
    rpc_server: &Option<Arc<ParkingRwLock<RpcServer>>>,
    wallet_provider: Option<&Arc<NodeWalletProvider>>,
    system: &Arc<NeoSystem>,
) -> Result<bool> {
    #[cfg(feature = "hsm")]
    {
        if !cli.hsm {
            return Ok(false);
        }

        if rpc_server.is_none() && wallet_provider.is_none() {
            warn!(
                target: "neo",
                "HSM requested but neither RPC nor wallet provider is active; skipping HSM wallet"
            );
            return Ok(false);
        }

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
        let wallet = crate::hsm_wallet::HsmWallet::from_runtime(
            runtime,
            Arc::new(system.settings().clone()),
        )
        .await
        .context("failed to build HSM wallet")?;
        let wallet_arc: Arc<dyn CoreWallet> = Arc::new(wallet);
        if let Some(provider) = wallet_provider {
            provider.set_wallet(Some(wallet_arc.clone()));
        }
        if !cli.rpc_hardened
            && let Some(server) = rpc_server
        {
            server.write().set_wallet(Some(wallet_arc));
        }
        Ok(true)
    }

    #[cfg(not(feature = "hsm"))]
    {
        let _ = (cli, node_config, rpc_server, wallet_provider, system);
        Ok(false)
    }
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
) -> Result<bool> {
    let Some((wallet_path, password)) = resolve_wallet_config(cli, node_config)? else {
        return Ok(false);
    };

    if rpc_server.is_none() && wallet_provider.is_none() {
        warn!(
            target: "neo",
            path = %wallet_path.display(),
            "wallet configured but neither RPC nor wallet provider is active; skipping wallet load"
        );
        return Ok(false);
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
    if !cli.rpc_hardened
        && let Some(server) = rpc_server
    {
        server.write().set_wallet(Some(wallet_arc.clone()));
    }

    if !cli.rpc_hardened && rpc_server.is_some() {
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
    Ok(true)
}
