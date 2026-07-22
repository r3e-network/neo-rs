//! Operator command validation and node-runtime opening.

use std::sync::Arc;

use anyhow::Context;
use neo_node::NodeLifecycleLock;
use tracing::info;

use super::runtime::{NodeRuntime, OpenNodeRuntime};
use crate::node::cli::{LedgerMode, NodeCli, validate_cli_mode};
use crate::node::config::{load_config, validate_config_for_ledger_mode};
use crate::node::lifecycle::composition::build_node;
use crate::node::lifecycle::preflight::{StartupPreflight, run_startup_preflight};
use crate::node::logging;
use crate::node::observability;

/// Validated operator request that has not opened runtime resources yet.
#[derive(Debug)]
pub(in crate::node) struct NodeCommand {
    cli: NodeCli,
}

impl NodeCommand {
    /// Validate an operator command without opening storage or starting tasks.
    pub(in crate::node) fn from_cli(cli: NodeCli) -> anyhow::Result<Self> {
        validate_cli_mode(&cli)?;
        Ok(Self { cli })
    }

    /// Load configuration, run preflight, and compose the node runtime.
    pub(in crate::node) async fn open_runtime(self) -> anyhow::Result<OpenNodeRuntime> {
        let ledger_mode = LedgerMode::from_cli(&self.cli);
        let (chain_spec, mut config) = load_config(&self.cli.config, self.cli.network_magic)?;
        let configured_stateroot = config.state_service.enabled;
        let configured_track_during_catchup = config.state_service.track_during_catchup;
        let stateroot_mode = self
            .cli
            .resolve_stateroot_mode(configured_stateroot, configured_track_during_catchup)?;
        config.state_service.enabled = stateroot_mode.enabled;
        config.state_service.track_during_catchup = stateroot_mode.track_during_catchup;
        let logging_guards = logging::init_tracing(&config.logging)?;
        let network_magic = chain_spec.network_magic();
        info!(
            target: "neo",
            chain = chain_spec.identity().name(),
            network = format_args!("0x{network_magic:08X}"),
            config = %self.cli.config.display(),
            stateroot_enabled = config.state_service.enabled,
            stateroot_configured = configured_stateroot,
            stateroot_track_during_catchup = config.state_service.track_during_catchup,
            stateroot_track_during_catchup_configured = configured_track_during_catchup,
            "loaded chain specification"
        );
        validate_config_for_ledger_mode(
            &config,
            network_magic,
            ledger_mode,
            self.cli.storage_path.as_deref(),
        )?;

        if run_startup_preflight(&self.cli, &config, network_magic, ledger_mode)?
            == StartupPreflight::Exit
        {
            return Ok(OpenNodeRuntime::Exit);
        }

        let lifecycle_lock = if ledger_mode.uses_local_replay_services() {
            self.cli
                .storage_path
                .clone()
                .or_else(|| config.storage.data_directory())
                .map(NodeLifecycleLock::acquire)
                .transpose()
                .context("acquiring exclusive node data-directory ownership")?
        } else {
            None
        };

        let observability =
            observability::ObservabilityRuntime::from_config(&config.observability, network_magic)?;
        if let Some(observability) = &observability {
            observability.install_panic_hook();
        }

        let startup_bulk_import = self.cli.import_chain.is_some() || self.cli.fast_sync;
        let running_node = match build_node(
            Arc::clone(&chain_spec),
            &config,
            self.cli.storage_path.as_deref(),
            self.cli.stop_at_height,
            ledger_mode,
            startup_bulk_import,
            observability.clone(),
        )
        .await
        {
            Ok(running_node) => running_node,
            Err(error) => {
                let error = error.context("failed to construct neo-system Node");
                if let Some(observability) = &observability {
                    observability.report_startup_error(&error);
                }
                return Err(error);
            }
        };
        info!(target: "neo", "neo-system Node built; blockchain service running");

        Ok(OpenNodeRuntime::Ready(NodeRuntime::new(
            self.cli,
            config,
            network_magic,
            logging_guards,
            observability,
            running_node,
            lifecycle_lock,
        )))
    }
}
