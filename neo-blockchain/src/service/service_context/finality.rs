//! Finalized block notification contract.
//!
//! This module owns the data that crosses the canonical durability boundary.
//! The blockchain service creates a notification only after the Ledger store
//! commits successfully. Upper layers may then derive non-consensus read
//! projections without reaching back into execution internals.

use std::sync::Arc;

use neo_payloads::{ApplicationExecuted, Block};
use neo_storage::{CacheRead, DataCache};

/// Observer semantics for the current block persistence call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlockPersistContext {
    /// Ordinary persistence; daemon hooks may derive catch-up behavior from
    /// the current peer tip for this individual block.
    Live,
    /// A range-level decision to retain live observer behavior for every block
    /// in a deferred sync batch, independent of later peer-tip changes.
    SyncBatch,
    /// A range-level catch-up decision frozen before a deferred sync batch.
    ///
    /// Live plugin staging is skipped for every block in the batch even if the
    /// observed peer tip changes while the batch is executing.
    CatchUp,
    /// Trusted local bootstrap/import such as `chain.acc` or built-in fast sync.
    TrustedReplay,
}

impl BlockPersistContext {
    /// Normal live-network/consensus persistence.
    #[must_use]
    pub const fn live() -> Self {
        Self::Live
    }

    /// Frozen catch-up observer semantics for a verified sync batch.
    #[must_use]
    pub const fn catch_up() -> Self {
        Self::CatchUp
    }

    /// Frozen live observer semantics for a verified sync batch.
    #[must_use]
    pub const fn sync_batch() -> Self {
        Self::SyncBatch
    }

    /// Trusted local bootstrap/import persistence.
    #[must_use]
    pub const fn trusted_replay() -> Self {
        Self::TrustedReplay
    }

    /// Returns whether live observer work must be skipped.
    #[must_use]
    pub const fn skips_live_observers(self) -> bool {
        matches!(self, Self::CatchUp | Self::TrustedReplay)
    }

    /// Returns whether daemon hooks may derive catch-up from the current peer tip.
    #[must_use]
    pub const fn uses_dynamic_peer_tip(self) -> bool {
        matches!(self, Self::Live)
    }

    /// Returns whether this is a trusted local replay path.
    #[must_use]
    pub const fn is_trusted_replay(self) -> bool {
        matches!(self, Self::TrustedReplay)
    }
}

/// Owned notification for one durably committed canonical block.
///
/// The block and execution records use shared ownership so a bounded consumer
/// stream can move them between tasks without cloning transaction bodies,
/// stacks, or notifications. When present, `snapshot` is the canonical cache
/// at the notification height. Observer-skipped deferred batches omit it
/// because their shared cache already represents the batch tip. The canonical
/// writer waits for acknowledgement before it starts another observer-visible
/// block, keeping a supplied view stable.
#[derive(Clone)]
pub struct FinalizedBlock<B>
where
    B: CacheRead,
{
    block: Arc<Block>,
    snapshot: Option<Arc<DataCache<B>>>,
    application_executed: Arc<[ApplicationExecuted]>,
    context: BlockPersistContext,
}

impl<B> FinalizedBlock<B>
where
    B: CacheRead,
{
    /// Creates a finalized notification from owned persistence artifacts.
    #[must_use]
    pub fn new(
        block: Arc<Block>,
        snapshot: Option<Arc<DataCache<B>>>,
        application_executed: Vec<ApplicationExecuted>,
        context: BlockPersistContext,
    ) -> Self {
        Self {
            block,
            snapshot,
            application_executed: Arc::from(application_executed),
            context,
        }
    }

    /// Returns the committed block without cloning its body.
    #[must_use]
    pub fn block(&self) -> &Arc<Block> {
        &self.block
    }

    /// Returns the canonical snapshot at this notification boundary.
    #[must_use]
    pub fn snapshot(&self) -> Option<&Arc<DataCache<B>>> {
        self.snapshot.as_ref()
    }

    /// Returns execution records in C# `allApplicationExecuted` order.
    #[must_use]
    pub fn application_executed(&self) -> &[ApplicationExecuted] {
        self.application_executed.as_ref()
    }

    /// Returns the persistence context frozen by the import path.
    #[must_use]
    pub const fn context(&self) -> BlockPersistContext {
        self.context
    }
}

impl<B> std::fmt::Debug for FinalizedBlock<B>
where
    B: CacheRead,
{
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FinalizedBlock")
            .field("height", &self.block.index())
            .field("transactions", &self.block.transactions.len())
            .field("application_executed", &self.application_executed.len())
            .field("has_snapshot", &self.snapshot.is_some())
            .field("context", &self.context)
            .finish()
    }
}
