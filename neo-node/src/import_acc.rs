//! Offline `.acc` block importer.
//!
//! The file format is:
//! - `u32` start index (little-endian)
//! - `u32` block count (little-endian)
//! - repeated `count` times:
//!   - `u32` block byte length
//!   - serialized `Block` payload bytes

use self::{env::import_stop_height_from_env, session::import_acc_file_until};
use crate::import_telemetry::{
    FastPathDeltas, ImportCounters, LogsDeltas, ParallelDeltas, PersistenceProfileDeltas,
};
use anyhow::Result;
use neo_system::NeoSystem;
use std::{path::Path, sync::Arc};

mod checkpoint;
mod env;
mod logs;
mod run;
mod session;

#[cfg(test)]
mod tests;

#[derive(Debug, Clone, Copy)]
pub struct ImportSummary {
    pub declared_start: u32,
    pub declared_count: u32,
    pub imported: u64,
    pub skipped: u64,
    pub final_height: u32,
    pub elapsed_secs: f64,
    /// Run-cumulative block/transaction counters. Skipped archive records are
    /// never counted here.
    pub counters: ImportCounters,
    /// Fast-path counter delta measured across this run.
    pub fast_path: FastPathDeltas,
    /// Speculative-scheduler counter delta measured across this run.
    pub parallel: ParallelDeltas,
    /// Opt-in persistence-profile delta measured across this run.
    pub profile: PersistenceProfileDeltas,
    /// ApplicationLogs write coalescing measured across this run.
    pub logs: LogsDeltas,
}

pub fn import_acc_file(
    system: &Arc<NeoSystem>,
    path: &Path,
    storage_path: Option<&str>,
) -> Result<ImportSummary> {
    import_acc_file_until(system, path, storage_path, import_stop_height_from_env())
}
