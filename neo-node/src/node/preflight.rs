//! Startup preflight checks and early-exit outcomes.
//!
//! This module keeps operator-requested config/storage validation out of the
//! daemon composition root. It does not decide CLI mode; it only executes the
//! preflight mode selected by `cli`.

use tracing::info;

use super::cli::{LedgerMode, NodeCli, StoragePreflightMode, storage_preflight_mode};
use super::config::{NodeConfig, validate_storage};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(in crate::node) enum StartupPreflight {
    Continue,
    Exit,
}

pub(in crate::node) fn run_startup_preflight(
    cli: &NodeCli,
    config: &NodeConfig,
    network_magic: u32,
    ledger_mode: LedgerMode<'_>,
) -> anyhow::Result<StartupPreflight> {
    let check_config = cli.check_config || cli.check_all;
    let storage_preflight = storage_preflight_mode(cli, ledger_mode);
    if check_config && storage_preflight == StoragePreflightMode::None {
        print_config_preflight_ok(cli);
        return Ok(StartupPreflight::Exit);
    }
    match storage_preflight {
        StoragePreflightMode::None => {}
        StoragePreflightMode::ValidateLocal => {
            validate_storage(config, cli.storage_path.as_deref(), network_magic)?;
            info!(target: "neo", config = %cli.config.display(), "storage preflight passed");
            println!("storage OK: {}", cli.config.display());
            return Ok(StartupPreflight::Exit);
        }
        StoragePreflightMode::SkipRemoteLedger => {
            info!(
                target: "neo::remote_ledger",
                config = %cli.config.display(),
                "storage preflight skipped; remote-ledger mode does not open a local canonical ledger"
            );
            println!(
                "storage skipped for remote ledger: {}",
                cli.config.display()
            );
            return Ok(StartupPreflight::Exit);
        }
    }

    if check_config {
        print_config_preflight_ok(cli);
        return Ok(StartupPreflight::Exit);
    }

    Ok(StartupPreflight::Continue)
}

fn print_config_preflight_ok(cli: &NodeCli) {
    info!(target: "neo", config = %cli.config.display(), "configuration preflight passed");
    println!("configuration OK: {}", cli.config.display());
}
