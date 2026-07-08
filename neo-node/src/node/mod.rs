//! # neo-node::node
//!
//! Daemon composition, CLI modes, and long-running node startup.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `chain_acc`: chain.acc import, reporting, and throughput accounting
//!   helpers.
//! - `cli`: Command-line arguments, ledger mode selection, and startup
//!   preflight policy.
//! - `config`: HSM provider configuration and signing profile records.
//! - `context`: Runtime context records carried through the local workflow.
//! - `daemon`: Top-level parse, preflight, composition, import, live-service,
//!   and shutdown orchestration.
//! - `fast_sync`: Built-in fast-sync package discovery, download, verification,
//!   and import flow.
//! - `indexer_runtime`: Indexer runtime wiring used by the node daemon.
//! - `inventory_relay`: Inbound peer-inventory batching and service forwarding.
//! - `ledger_source`: Local and remote ledger source abstractions used by node
//!   modes.
//! - `live_services`: Post-import telemetry, P2P, seed dialing, RPC, and
//!   observability heartbeat startup.
//! - `logging`: Logging, tracing, and operator diagnostics setup.
//! - `observability`: Metrics and observability endpoint wiring.
//! - `preflight`: Startup config/storage preflight checks and early-exit
//!   outcomes.
//! - `remote_ledger`: RPC-backed ledger source used when the node runs without
//!   a local ledger.
//! - `rpc_runtime`: RPC server runtime wiring and shutdown handling.
//! - `seeds`: Seed-node selection and network bootstrap helpers.
//! - `services`: Auxiliary service startup and handles used by the daemon.
//! - `shutdown`: OS, stop-height, and essential-task shutdown waiting.
//! - `shutdown_flow`: Daemon shutdown cancellation, task abort, and durable
//!   store finalization.
//! - `startup_cleanup`: Startup import rollback, durable-mode restore, and
//!   shutdown flush helpers.
//! - `sync_downloader`: Coordinator-backed P2P block download startup.
//! - `sync_metrics`: Sync-speed counters, summaries, and operator-facing
//!   throughput status.
//! - `tasks`: Task supervision, shutdown wiring, and background-service
//!   handles.
//! - `telemetry`: Telemetry startup and reporting helpers.
//! - `tests`: Module-local tests and regression coverage.

#[cfg(test)]
use clap::Parser;
#[cfg(test)]
use neo_config::ProtocolSettings;
#[cfg(test)]
use std::path::Path;
#[cfg(test)]
use std::sync::Arc;

mod chain_acc;
mod cli;
mod composition;
mod config;
mod context;
mod daemon;
mod fast_sync;
mod indexer_runtime;
mod inventory_relay;
mod ledger_source;
mod live_services;
mod logging;
mod observability;
mod preflight;
mod remote_ledger;
mod rpc_payload;
mod rpc_runtime;
mod seeds;
mod services;
mod shutdown;
mod shutdown_flow;
mod startup_cleanup;
mod startup_import;
mod sync_downloader;
mod sync_metrics;
mod tasks;
mod telemetry;

#[cfg(test)]
use cli::NodeCli;
#[cfg(test)]
use cli::import_tip_reaches_stop_height;
#[cfg(test)]
use cli::{LedgerMode, validate_cli_mode};
#[cfg(test)]
use cli::{StoragePreflightMode, storage_preflight_mode};
#[cfg(test)]
use composition::build_node;
#[cfg(test)]
use config::default_p2p_port;
#[cfg(test)]
use config::validate_storage;
#[cfg(test)]
use config::{NodeConfig, open_store, validate_config};
#[cfg(test)]
use config::{load_config, validate_config_for_ledger_mode};
#[cfg(test)]
use context::DaemonContext;
#[cfg(test)]
use inventory_relay::{FAST_SYNC_BURST_CAPACITY, flush_inventory_block_batch};
#[cfg(test)]
use rpc_runtime::start_rpc_server;
#[cfg(test)]
use startup_cleanup::abort_fast_sync_store_mode;
#[cfg(test)]
use startup_cleanup::{flush_state_service_for_shutdown, restore_durable_store_mode};

/// Entry point: parse CLI, load config, build the node, start P2P +
/// RPC, and wait for `Ctrl-C`.
pub async fn run() -> anyhow::Result<()> {
    daemon::run().await
}

#[cfg(test)]
#[path = "../tests/node/mod.rs"]
mod tests;
