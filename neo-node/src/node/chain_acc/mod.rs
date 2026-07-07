//! # neo-node::node::chain_acc
//!
//! chain.acc import, reporting, and throughput accounting helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. This application crate may compose lower
//! layers but must not define protocol bytes, storage formats, consensus rules,
//! or VM semantics.
//!
//! ## Contents
//!
//! - `batch`: batch accounting and dispatch helpers.
//! - `driver`: Stream import orchestration and report assembly.
//! - `format`: chain.acc file format readers and validation helpers.
//! - `metrics`: Metrics collection and progress-reporting helpers.
//! - `range`: Expected-range, resume, and continuity validation helpers.
//! - `report`: Import report DTOs and hot-metric projection.

mod batch;
mod driver;
mod format;
mod metrics;
mod range;
mod report;

pub use driver::import_chain_acc_until_height;
pub(super) use driver::{import_chain_acc_report_with_expected_range, local_ledger_tip};
pub(super) use report::{ChainAccImportReport, ImportHotMetrics, LocalLedgerTip};

/// The mixed-block batch size for trusted `chain.acc` Import commands.
///
/// C# Neo uses 10 because it prioritizes simple live-import parity. This path is
/// a trusted local fast-sync import: larger mixed batches reduce expensive
/// StateService/durable-store finalization fences while preserving per-block
/// native/state transitions. Empty-only runs use the same outer command
/// boundary: the blockchain service owns the smaller internal empty
/// fast-forward chunks while keeping one outer batch snapshot/finalization.
const IMPORT_BATCH_SIZE: usize = 10_000;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct ChainAccExpectedRange {
    pub(super) start_height: u32,
    pub(super) end_height: u32,
}

#[cfg(test)]
mod tests;
