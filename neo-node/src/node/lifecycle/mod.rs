//! # neo-node::node::lifecycle
//!
//! Node startup, composition, live operation, and graceful shutdown.
//!
//! ## Boundary
//!
//! This application layer sequences lower-level services and durability
//! policies. It must not implement protocol validation, VM semantics, storage
//! formats, or P2P wire behavior.
//!
//! ## Contents
//!
//! - `composition`: provider and service graph construction.
//! - `daemon`: process-level node entry workflow.
//! - `live_services`: post-import RPC, P2P, telemetry, and seed startup.
//! - `preflight`: startup configuration and storage checks.
//! - `shutdown`: shutdown signal and stop-height waiting.
//! - `shutdown_flow`: task cancellation and durable finalization.
//! - `startup_cleanup`: failed-import and fast-sync cleanup.
//! - `startup_import`: chain.acc and fast-sync startup imports.
//! - `workflow`: high-level running-node mode orchestration.

use super::{
    application, chain_acc, cli, config, context, fast_sync, indexer_runtime, inventory_relay,
    ledger_source, observability, recovery, remote_ledger, rpc_runtime, seeds, services,
    static_files, sync_downloader, tasks, telemetry,
};

pub(super) mod composition;
pub(super) mod daemon;
pub(super) mod live_services;
pub(super) mod preflight;
pub(super) mod shutdown;
pub(super) mod shutdown_flow;
pub(super) mod startup_cleanup;
pub(super) mod startup_import;
pub(super) mod workflow;
