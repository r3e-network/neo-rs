//! Daemon startup and lifetime orchestration.
//!
//! This module owns the ordered top-level workflow for the `neo-node` binary:
//! parse CLI input, load and validate configuration, run startup preflight,
//! compose the system node, execute startup imports, start live services, and
//! enter graceful shutdown. Lower modules own the mechanics for each step.

use std::sync::Arc;

use clap::Parser;
use tracing::info;

use super::cli::{LedgerMode, NodeCli, validate_cli_mode};
use super::composition::{RunningNode, build_node};
use super::config::{load_config, validate_config_for_ledger_mode};
use super::live_services::start_live_services;
use super::preflight::{StartupPreflight, run_startup_preflight};
use super::shutdown_flow::run_daemon_shutdown;
use super::startup_import::{StartupImportContext, StartupImportOutcome, run_startup_imports};
use super::{logging, observability};

pub(super) async fn run() -> anyhow::Result<()> {
    let cli = NodeCli::parse();
    validate_cli_mode(&cli)?;
    let ledger_mode = LedgerMode::from_cli(&cli);
    let (settings, config) = load_config(&cli.config, cli.network_magic)?;
    let _logging_guards = logging::init_tracing(&config.logging)?;
    let settings = Arc::new(settings);
    info!(
        target: "neo",
        network = format_args!("0x{:08X}", settings.network),
        config = %cli.config.display(),
        "loaded protocol settings"
    );
    validate_config_for_ledger_mode(&config, settings.network, ledger_mode)?;

    if run_startup_preflight(&cli, &config, settings.network, ledger_mode)?
        == StartupPreflight::Exit
    {
        return Ok(());
    }

    let observability =
        observability::ObservabilityRuntime::from_config(&config.observability, settings.network)?;
    if let Some(observability) = &observability {
        observability.install_panic_hook();
    }

    let running_node = match build_node(
        Arc::clone(&settings),
        &config,
        cli.storage_path.as_deref(),
        cli.stop_at_height,
        ledger_mode,
        cli.import_chain.is_some() || cli.fast_sync,
        observability.clone(),
    )
    .await
    {
        Ok(running_node) => running_node,
        Err(err) => {
            let err = err.context("failed to construct neo-system Node");
            if let Some(observability) = &observability {
                observability.report_startup_error(&err);
            }
            return Err(err);
        }
    };
    let RunningNode {
        node,
        network,
        mut handles,
        shutdown,
        durable_service_stores,
    } = running_node;
    info!(target: "neo", "neo-system Node built; blockchain service running");

    let startup_import = run_startup_imports(StartupImportContext {
        cli: &cli,
        node: &node,
        config: &config,
        network: settings.network,
        durable_service_stores: &durable_service_stores,
        handles: &mut handles,
        observability: observability.as_ref(),
    })
    .await?;
    if startup_import == StartupImportOutcome::StopHeightReached {
        return Ok(());
    }

    let _live_service_guards = start_live_services(
        &node,
        &network,
        &mut handles,
        &shutdown,
        &config,
        settings.network,
        ledger_mode,
        observability.as_ref(),
    )
    .await?;

    run_daemon_shutdown(
        &node,
        cli.stop_at_height,
        shutdown,
        handles,
        &durable_service_stores,
        observability.as_ref(),
    )
    .await
}
