//! Bounded pack-store configuration.

use anyhow::{Result, ensure};

use super::super::read_view::PACK_BATCH_VALUES_PER_WORKER;

/// Physical read-path options that do not change pack bytes or lookup
/// semantics. Every accelerator is disabled by default.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PackStoreOptions {
    /// Map immutable pack and index files a second time with `MADV_RANDOM`.
    /// All index-located payloads and sparse index-window probes use that view;
    /// compaction, validation, and scrub keep the ordinary mapping.
    pub random_point_mmap: bool,
    /// Workers used to copy values for large sorted batch reads. A value of
    /// one keeps the sequential path. Values above one split only immutable
    /// payload reads; index lookup and result publication remain ordered.
    pub batch_value_workers: usize,
}

impl PackStoreOptions {
    /// Configured worker count capped by the logical CPUs visible to this
    /// process. Failure to query the host fails closed to the sequential path.
    pub fn effective_batch_value_workers(self) -> usize {
        let available = std::thread::available_parallelism().map_or(1, usize::from);
        self.batch_value_workers.min(available)
    }

    /// Minimum number of located values required before parallel copying is
    /// worthwhile for this configuration.
    pub fn batch_value_parallel_threshold(self) -> usize {
        self.effective_batch_value_workers()
            .saturating_mul(PACK_BATCH_VALUES_PER_WORKER)
    }

    pub(in crate::engine::store) fn normalized_for_host(self) -> Self {
        Self {
            random_point_mmap: self.random_point_mmap,
            batch_value_workers: self.effective_batch_value_workers(),
        }
    }
}

impl Default for PackStoreOptions {
    fn default() -> Self {
        Self {
            random_point_mmap: false,
            batch_value_workers: 1,
        }
    }
}

/// Leveled compaction bounds for the derived index runs. Level 0 holds the
/// most recent append runs; when a level exceeds its run bound the oldest
/// runs (up to `fanout`) merge into one run at the next level. Payload
/// frames are never rewritten; compacted records keep pointing at the
/// original frame bytes.
#[derive(Clone, Copy, Debug)]
pub(crate) struct CompactionConfig {
    pub(in crate::engine::store) l0_bound: usize,
    pub(in crate::engine::store) l1_bound: usize,
    pub(in crate::engine::store) fanout: usize,
}

impl Default for CompactionConfig {
    /// Every level holds at most 8 runs; one cycle merges up to 16 inputs.
    fn default() -> Self {
        Self {
            l0_bound: 8,
            l1_bound: 8,
            fanout: 16,
        }
    }
}

pub(in crate::engine::store) fn validate_compaction_config(config: CompactionConfig) -> Result<()> {
    ensure!(
        config.l0_bound >= 1 && config.l1_bound >= 1,
        "compaction level bounds must be non-zero"
    );
    ensure!(config.fanout >= 2, "compaction fanout must exceed one");
    Ok(())
}

pub(in crate::engine::store) fn validate_store_options(options: PackStoreOptions) -> Result<()> {
    ensure!(
        (1..=8).contains(&options.batch_value_workers),
        "batch value workers must be in 1..=8"
    );
    Ok(())
}
