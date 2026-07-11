//! Operator command validation and node-runtime opening.

use std::sync::Arc;

use tracing::info;

use super::runtime::{NodeRuntime, OpenNodeRuntime};
use crate::node::cli::{LedgerMode, NodeCli, validate_cli_mode};
use crate::node::composition::build_node;
use crate::node::config::{load_config, validate_config_for_ledger_mode};
use crate::node::logging;
use crate::node::observability;
use crate::node::preflight::{StartupPreflight, run_startup_preflight};

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
        let (settings, config) = load_config(&self.cli.config, self.cli.network_magic)?;
        let logging_guards = logging::init_tracing(&config.logging)?;
        let settings = Arc::new(settings);
        info!(
            target: "neo",
            network = format_args!("0x{:08X}", settings.network),
            config = %self.cli.config.display(),
            "loaded protocol settings"
        );
        validate_config_for_ledger_mode(
            &config,
            settings.network,
            ledger_mode,
            self.cli.storage_path.as_deref(),
        )?;

        if run_startup_preflight(&self.cli, &config, settings.network, ledger_mode)?
            == StartupPreflight::Exit
        {
            return Ok(OpenNodeRuntime::Exit);
        }

        let observability = observability::ObservabilityRuntime::from_config(
            &config.observability,
            settings.network,
        )?;
        if let Some(observability) = &observability {
            observability.install_panic_hook();
        }

        let startup_bulk_import = self.cli.import_chain.is_some() || self.cli.fast_sync;
        let running_node = match build_node(
            Arc::clone(&settings),
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
            settings.network,
            logging_guards,
            observability,
            running_node,
        )))
    }
}
