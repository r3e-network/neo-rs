//! Command-line arguments and startup mode selection.
//!
//! This module owns the CLI-facing daemon contract and the small preflight
//! decisions derived from it. Long-running startup, service composition, and
//! protocol behavior stay in the surrounding `node` module and lower crates.

use std::path::PathBuf;

use clap::Parser;

/// Default path to the node configuration file.
pub const DEFAULT_SETTINGS_PATH: &str = "neo_testnet_node.toml";

/// Command-line arguments for the `neo-node` daemon.
#[derive(Debug, Parser)]
#[command(name = "neo-node", version, about = "Neo N3 node daemon")]
pub struct NodeCli {
    /// Path to the TOML node configuration file.
    #[arg(long, short = 'c', default_value = DEFAULT_SETTINGS_PATH)]
    pub config: PathBuf,

    /// Override the network magic advertised in the protocol settings
    /// (must match the rest of the network).
    #[arg(long)]
    pub network_magic: Option<u32>,

    /// Override the persistent storage directory. Uses the configured
    /// persistent backend, or the build's default persistent backend.
    #[arg(long)]
    pub storage_path: Option<PathBuf>,

    /// Validate the node configuration and exit without starting services.
    #[arg(long)]
    pub check_config: bool,

    /// Validate the configured storage backend can be opened and exit.
    #[arg(long)]
    pub check_storage: bool,

    /// Run all preflight checks and exit.
    #[arg(long)]
    pub check_all: bool,

    /// Import blocks from a chain.acc dump file before starting the node.
    /// The file is the C# Neo block-dump format (u32 count, then repeated
    /// i32-size + serialized-Block). Blocks are imported with verify=false
    /// (trusted source, like C# Neo's chain.acc import). After import, the
    /// node starts normally and continues syncing from the network.
    #[arg(long, value_name = "PATH")]
    pub import_chain: Option<PathBuf>,

    /// Download and import the official NGD N3 fast-sync package before
    /// starting network sync. The package URL is resolved from the built-in
    /// official manifest URL and cached locally after MD5 validation.
    #[arg(long)]
    pub fast_sync: bool,

    /// Override the directory used to cache the official fast-sync package.
    #[arg(long, value_name = "PATH", requires = "fast_sync")]
    pub fast_sync_cache: Option<PathBuf>,

    /// Validate the imported fast-sync block tip against an upstream JSON-RPC
    /// endpoint before clearing the fast-sync import marker.
    #[arg(long, value_name = "URL", requires = "fast_sync")]
    pub fast_sync_reference_rpc: Option<String>,

    /// Write a machine-readable fast-sync import proof JSON after a successful
    /// package import.
    #[arg(long, value_name = "PATH", requires = "fast_sync")]
    pub fast_sync_report: Option<PathBuf>,

    /// Stop gracefully after this persisted block height is reached.
    #[arg(long, value_name = "HEIGHT")]
    pub stop_at_height: Option<u32>,

    /// Run without a local canonical ledger and delegate ledger/state/indexer
    /// reads plus relay-style RPC calls to an upstream JSON-RPC endpoint.
    #[arg(long, value_name = "URL")]
    pub remote_ledger_rpc: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum LedgerMode<'a> {
    Local,
    RemoteRpc { endpoint: &'a str },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum StoragePreflightMode {
    None,
    ValidateLocal,
    SkipRemoteLedger,
}

impl<'a> LedgerMode<'a> {
    pub(super) fn from_cli(cli: &'a NodeCli) -> Self {
        cli.remote_ledger_rpc
            .as_deref()
            .map(|endpoint| Self::RemoteRpc { endpoint })
            .unwrap_or(Self::Local)
    }

    pub(super) fn remote_endpoint(self) -> Option<&'a str> {
        match self {
            Self::Local => None,
            Self::RemoteRpc { endpoint } => Some(endpoint),
        }
    }

    pub(super) fn uses_local_replay_services(self) -> bool {
        matches!(self, Self::Local)
    }
}

pub(super) fn storage_preflight_mode(
    cli: &NodeCli,
    ledger_mode: LedgerMode<'_>,
) -> StoragePreflightMode {
    if !(cli.check_storage || cli.check_all) {
        return StoragePreflightMode::None;
    }
    if ledger_mode.uses_local_replay_services() {
        StoragePreflightMode::ValidateLocal
    } else {
        StoragePreflightMode::SkipRemoteLedger
    }
}

pub(super) fn validate_cli_mode(cli: &NodeCli) -> anyhow::Result<()> {
    if cli.remote_ledger_rpc.is_some() && (cli.import_chain.is_some() || cli.fast_sync) {
        anyhow::bail!(
            "--remote-ledger-rpc runs without a local canonical ledger; do not combine it with --import-chain or --fast-sync"
        );
    }
    Ok(())
}

pub(super) fn import_tip_reaches_stop_height(
    imported_tip: u32,
    stop_at_height: Option<u32>,
) -> bool {
    stop_at_height.is_some_and(|target| imported_tip >= target)
}
