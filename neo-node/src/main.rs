//! Neo Node - Neo N3 node daemon (server)
//!
//! `neo-node` is a long-running daemon: it runs the Neo N3 protocol, syncs the chain over P2P,
//! and (optionally) exposes a JSON-RPC server for external clients.

#![warn(missing_docs)]

mod cli;
mod config;
mod consensus;
mod health;
#[cfg(feature = "hsm")]
mod hsm_integration;
#[cfg(feature = "hsm")]
mod hsm_wallet;
#[cfg(feature = "full")]
mod import_acc;
#[cfg(not(feature = "full"))]
#[allow(missing_docs)]
mod import_acc {
    use anyhow::{bail, Result};
    use neo_core::neo_system::NeoSystem;
    use std::{path::Path, sync::Arc};

    #[derive(Debug, Clone, Copy)]
    pub struct ImportSummary {
        pub declared_start: u32,
        pub declared_count: u32,
        pub imported: u64,
        pub skipped: u64,
        pub final_height: u32,
        pub elapsed_secs: f64,
    }

    pub fn import_acc_file(
        _system: &Arc<NeoSystem>,
        path: &Path,
        _storage_path: Option<&str>,
    ) -> Result<ImportSummary> {
        bail!(
            "ACC import requires neo-node full feature support because it verifies RocksDB checkpoints: {}",
            path.display()
        )
    }
}
mod logging;
mod metrics;
mod rpc_consensus;
mod startup;
#[cfg(feature = "tee")]
mod tee_integration;
#[cfg(feature = "tee")]
mod tee_wallet;
mod wallet_provider;

use anyhow::Result;
use clap::Parser;
use cli::NodeCli;
use tracing::info;

#[global_allocator]
static GLOBAL_ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> Result<()> {
    let cli = NodeCli::parse();
    maybe_enable_import_batch_profile(&cli);

    let rt = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(num_cpus::get().max(4))
        .max_blocking_threads(512)
        .global_queue_interval(61)
        .enable_all()
        .thread_name("neo-node")
        .thread_stack_size(2 * 1024 * 1024)
        .build()?;

    rt.block_on(startup::run(cli))
}

fn maybe_enable_import_batch_profile(cli: &NodeCli) {
    if cli.import_acc.is_none() {
        return;
    }
    if std::env::var_os("NEO_ROCKSDB_BATCH_PROFILE").is_some() {
        return;
    }

    set_env_var("NEO_ROCKSDB_BATCH_PROFILE", "high_throughput");
    info!(
        target: "neo",
        profile = "high_throughput",
        "auto-selected RocksDB high-throughput batch profile for --import-acc (set NEO_ROCKSDB_BATCH_PROFILE to override)"
    );
}

fn set_env_var<K: AsRef<std::ffi::OsStr>, V: AsRef<std::ffi::OsStr>>(key: K, value: V) {
    // SAFETY: This runs during process startup before the Tokio runtime and
    // worker threads are created, and only sets neo-node's own default profile.
    #[allow(unused_unsafe)]
    unsafe {
        std::env::set_var(key, value);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use clap::Parser;
    use neo_core::{
        i_event_handlers::WalletChangedHandler,
        neo_system::NeoSystem,
        protocol_settings::ProtocolSettings,
        wallets::{WalletProvider, Wallet as CoreWallet},
    };
    use std::any::Any;
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    #[cfg(feature = "tee")]
    use tempfile::NamedTempFile;

    #[derive(Default)]
    struct WalletChangeProbe {
        changes: AtomicUsize,
    }

    impl WalletChangedHandler for WalletChangeProbe {
        fn wallet_provider_wallet_changed_handler(
            &self,
            _sender: &dyn Any,
            _wallet: Option<Arc<dyn CoreWallet>>,
        ) {
            self.changes.fetch_add(1, Ordering::SeqCst);
        }
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn setup_wallet_provider_works_without_rpc_server() {
        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
        let probe = Arc::new(WalletChangeProbe::default());
        system
            .register_wallet_changed_handler(probe.clone())
            .expect("register probe");

        let provider =
            crate::startup::services::setup_wallet_provider(&None, &system, true)
                .expect("setup provider");

        assert!(provider.is_some());
        assert!(
            probe.changes.load(Ordering::SeqCst) >= 1,
            "wallet change probe should observe provider attachment"
        );

        system.shutdown().await.expect("shutdown");
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn maybe_open_wallet_uses_provider_without_rpc_server() {
        let cli = NodeCli::parse_from(["neo-node"]);
        let tmp = tempfile::TempDir::new().expect("temp dir");
        let missing_wallet = tmp.path().join("missing-wallet.json");

        let mut node_config = crate::config::NodeConfig::default();
        node_config.unlock_wallet.is_active = true;
        node_config.unlock_wallet.path = Some(missing_wallet.to_string_lossy().to_string());
        node_config.unlock_wallet.password = Some("password".to_string());

        let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("neo system");
        let provider =
            crate::startup::services::setup_wallet_provider(&None, &system, true)
                .expect("setup provider")
                .expect("provider");

        let err =
            crate::startup::services::maybe_open_wallet(
                &cli, &node_config, &None, Some(&provider), &system,
            )
            .expect_err("missing wallet file should fail");
        assert!(
            err.to_string().contains("wallet file not found"),
            "unexpected error: {err}"
        );
        assert!(
            provider.get_wallet().is_none(),
            "wallet should remain unset"
        );

        system.shutdown().await.expect("shutdown");
    }

    #[cfg(feature = "tee")]
    #[test]
    fn cli_accepts_tee_auto_mode() {
        let cli = NodeCli::parse_from(["neo-node", "--tee-auto"]);
        assert!(cli.tee_auto);
        assert!(!cli.tee);
    }

    #[cfg(feature = "tee")]
    #[test]
    fn cli_rejects_conflicting_tee_modes() {
        let result = NodeCli::try_parse_from(["neo-node", "--tee", "--tee-auto"]);
        assert!(result.is_err(), "expected --tee and --tee-auto to conflict");
    }

    #[cfg(feature = "tee")]
    #[tokio::test(flavor = "multi_thread")]
    async fn tee_auto_falls_back_when_tee_startup_fails() {
        let marker = NamedTempFile::new().expect("marker file");
        let tee_path = marker.path().to_string_lossy().to_string();
        let args = vec![
            "neo-node".to_string(),
            "--tee-auto".to_string(),
            "--tee-data-path".to_string(),
            tee_path,
        ];
        let cli = NodeCli::parse_from(args);
        let node_config = crate::config::NodeConfig::default();
        let protocol_settings = node_config.protocol_settings();
        let system = NeoSystem::new(protocol_settings.clone(), None, None).expect("neo system");

        let runtime =
            crate::startup::services::maybe_enable_tee_runtime(
                &cli, &node_config, &protocol_settings, &system,
            )
            .expect("tee-auto should fall back to non-TEE mode");
        assert!(runtime.is_none(), "tee-auto should continue without TEE");

        system.shutdown().await.expect("shutdown");
    }

    #[cfg(feature = "tee")]
    #[tokio::test(flavor = "multi_thread")]
    async fn tee_required_fails_when_tee_startup_fails() {
        let marker = NamedTempFile::new().expect("marker file");
        let tee_path = marker.path().to_string_lossy().to_string();
        let args = vec![
            "neo-node".to_string(),
            "--tee".to_string(),
            "--tee-data-path".to_string(),
            tee_path,
        ];
        let cli = NodeCli::parse_from(args);
        let node_config = crate::config::NodeConfig::default();
        let protocol_settings = node_config.protocol_settings();
        let system = NeoSystem::new(protocol_settings.clone(), None, None).expect("neo system");

        let result =
            crate::startup::services::maybe_enable_tee_runtime(
                &cli, &node_config, &protocol_settings, &system,
            );
        assert!(
            result.is_err(),
            "strict tee mode should fail when TEE startup fails"
        );

        system.shutdown().await.expect("shutdown");
    }

    #[test]
    fn cli_storage_override_updates_config_storage_path() {
        let cli = NodeCli::parse_from(["neo-node", "--storage", "/tmp/neo-custom"]);
        let mut node_config = crate::config::NodeConfig::default();

        crate::startup::cli::apply_cli_overrides(&cli, &mut node_config);

        assert_eq!(node_config.storage.path.as_deref(), Some("/tmp/neo-custom"));
    }
}
