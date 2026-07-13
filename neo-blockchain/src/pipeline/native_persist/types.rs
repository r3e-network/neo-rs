//! Native persistence resource and result types.
//!
//! The main persist module owns execution order. This module owns the durable
//! data passed between that order and callers: reusable provider captures,
//! staged snapshots, and replay outcomes.

use std::sync::Arc;

use tracing::debug;

use neo_execution::native_contract_provider::NativeContractProvider;
use neo_payloads::ApplicationExecuted;
use neo_storage::{CacheRead, DataCache};

use super::artifacts::NativePersistNotification;

/// Outcome of `persist_block_natives_with_resources` for one block.
#[derive(Debug, Clone, Default)]
pub struct NativePersistOutcome {
    /// Names of the native contracts whose `initialize()` ran at this block
    /// (their activation block is this block).
    pub initialized: Vec<String>,
    /// Per-engine execution records, in C# `Blockchain.Persist` order: the
    /// `OnPersist` engine, one entry per block transaction, then the
    /// `PostPersist` engine (C# `allApplicationExecuted`).
    pub application_executed: Vec<ApplicationExecuted>,
    /// Notifications emitted by the `OnPersist` native hooks.
    pub on_persist_notifications: Vec<NativePersistNotification>,
    /// Notifications emitted by the `PostPersist` native hooks.
    pub post_persist_notifications: Vec<NativePersistNotification>,
}

/// Controls which non-consensus replay artifacts native persistence materializes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct NativePersistOptions {
    /// Capture `ApplicationExecuted` records and cloned native notifications for
    /// indexer/finalized-projection replay. Disable for trusted local replay or
    /// when the concrete application composition has no artifact consumer.
    /// This does not skip protocol execution or durable state changes.
    pub capture_replay_artifacts: bool,
}

impl Default for NativePersistOptions {
    fn default() -> Self {
        Self {
            capture_replay_artifacts: true,
        }
    }
}

/// Reusable native-persistence resources for a sequence of blocks that share
/// the same explicit native-contract provider and protocol settings.
#[derive(Clone)]
pub struct NativePersistResources<P>
where
    P: NativeContractProvider + 'static,
{
    pub(super) provider: Arc<P>,
    pub(super) contracts: Arc<[P::Contract]>,
}

impl<P> NativePersistResources<P>
where
    P: NativeContractProvider + 'static,
{
    /// Captures the canonical native-contract list once from an explicit
    /// provider. The list order is the C# native registration order used by both
    /// OnPersist and PostPersist hooks.
    pub fn from_provider(provider: Arc<P>) -> Self {
        let contracts = Arc::from(provider.all_native_contracts());
        Self {
            provider,
            contracts,
        }
    }

    /// Returns the canonical native contracts captured for this persistence
    /// batch, in C# registration order.
    pub fn contracts(&self) -> &[P::Contract] {
        self.contracts.as_ref()
    }

    /// Returns the native-contract provider captured for this persistence batch.
    pub fn provider(&self) -> Arc<P> {
        Arc::clone(&self.provider)
    }
}

/// A block persistence result whose storage writes are still staged in a child
/// cache. The caller must run committing hooks against [`Self::snapshot`] and
/// call [`Self::commit`] only after every pre-commit gate succeeds.
pub struct StagedNativePersist<B: CacheRead> {
    /// The staged block writes, isolated from the canonical snapshot until
    /// [`Self::commit`] is called.
    pub(super) snapshot: Arc<DataCache<B>>,
    /// Native persistence metadata and ApplicationExecuted records.
    pub outcome: NativePersistOutcome,
    pub(super) block_index: u32,
    pub(super) n_tx: usize,
    pub(super) onpersist_us: u64,
    pub(super) tx_us: u64,
    pub(super) postpersist_us: u64,
    pub(super) total_start: std::time::Instant,
}

impl<B: CacheRead> StagedNativePersist<B> {
    /// Returns the staged snapshot that committing hooks should inspect.
    pub fn snapshot(&self) -> &DataCache<B> {
        self.snapshot.as_ref()
    }

    /// Publishes the staged writes into the canonical parent snapshot.
    pub fn commit(&self) {
        let cache_commit_start = std::time::Instant::now();
        self.snapshot.commit();
        let cache_commit_us = neo_runtime::time::elapsed_us(cache_commit_start.elapsed());
        let total_us = neo_runtime::time::elapsed_us(self.total_start.elapsed());
        neo_runtime::sync_metrics::record_native_persist(
            self.block_index as u64,
            self.n_tx as u64,
            self.onpersist_us,
            self.tx_us,
            self.postpersist_us,
            cache_commit_us,
            total_us,
        );
        debug!(
            target: "neo::sync",
            index = self.block_index,
            txs = self.n_tx,
            onpersist_us = self.onpersist_us,
            tx_stage_us = self.tx_us,
            postpersist_us = self.postpersist_us,
            cache_commit_us,
            total_us,
            "persist_block_natives timing"
        );
    }
}
