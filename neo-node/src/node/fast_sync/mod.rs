//! # neo-node::node::fast_sync
//!
//! Built-in fast-sync package discovery, download, verification, and import
//! flow.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `cache_dir`: Operator cache-directory resolution.
//! - `local`: Local ledger and StateService verification for fast-sync imports.
//! - `marker`: Crash-safety marker handling for in-progress imports.
//! - `orchestration`: End-to-end package import orchestration.
//! - `package`: Fast-sync package metadata, cache, and archive helpers.
//! - `reference`: Reference RPC verification helpers for fast-sync imports.
//! - `report`: Machine-readable import reports and throughput classification.

mod cache_dir;
mod local;
mod marker;
mod orchestration;
mod package;
mod reference;
mod report;

pub(super) use orchestration::run_fast_sync_report;
pub(super) use report::write_fast_sync_report_sidecar;

#[cfg(test)]
#[path = "../../tests/node/fast_sync/mod.rs"]
mod tests;
