//! Resolved execution policy for one block-import request.
//!
//! `ImportMode` captures caller intent. `ImportPlan` combines that intent with
//! the composition root's range-aware sync policy before any block is staged,
//! so validation, durability, observer behavior, and post-commit publication
//! remain consistent for the whole request.

use neo_payloads::Block;

use crate::import::ImportMode;
use crate::native_persist::NativePersistOptions;
use crate::service_context::{BlockPersistContext, SyncBatchCommitPolicy, SystemContext};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportDurability {
    PerBlock,
    DeferredBatch,
}

/// Immutable policy resolved before an import request mutates canonical state.
#[derive(Debug, Clone, Copy)]
pub(super) struct ImportPlan {
    mode: ImportMode,
    durability: ImportDurability,
    persist_context: BlockPersistContext,
}

impl ImportPlan {
    /// Resolve caller intent against the active composition's sync-batch policy.
    pub(super) fn resolve<S: SystemContext>(
        mode: ImportMode,
        blocks: &[Block],
        durable_height: u32,
        system: &S,
    ) -> Self {
        match mode {
            ImportMode::Live { .. } => Self {
                mode,
                durability: ImportDurability::PerBlock,
                persist_context: BlockPersistContext::live(),
            },
            ImportMode::TrustedReplay { .. } => Self {
                mode,
                durability: ImportDurability::DeferredBatch,
                persist_context: BlockPersistContext::trusted_replay(),
            },
            ImportMode::Sync => {
                let start_height = durable_height.saturating_add(1);
                let policy = blocks
                    .last()
                    .map(Block::index)
                    .filter(|end_height| start_height <= *end_height)
                    .map_or(SyncBatchCommitPolicy::PerBlock, |end_height| {
                        system.sync_batch_commit_policy(start_height, end_height)
                    });
                match policy {
                    SyncBatchCommitPolicy::PerBlock => Self {
                        mode,
                        durability: ImportDurability::PerBlock,
                        persist_context: BlockPersistContext::live(),
                    },
                    SyncBatchCommitPolicy::DeferredLive => Self {
                        mode,
                        durability: ImportDurability::DeferredBatch,
                        persist_context: BlockPersistContext::sync_batch(),
                    },
                    SyncBatchCommitPolicy::DeferredCatchUp => Self {
                        mode,
                        durability: ImportDurability::DeferredBatch,
                        persist_context: BlockPersistContext::catch_up(),
                    },
                }
            }
        }
    }

    #[inline]
    pub(super) const fn verify(self) -> bool {
        self.mode.verify()
    }

    #[inline]
    pub(super) const fn is_trusted_replay(self) -> bool {
        self.mode.is_trusted_replay()
    }

    #[inline]
    pub(super) const fn defers_store_commit(self) -> bool {
        matches!(self.durability, ImportDurability::DeferredBatch)
    }

    #[inline]
    pub(super) const fn allows_replay_artifacts(self) -> bool {
        self.mode.allows_replay_artifacts()
    }

    #[inline]
    pub(super) const fn persist_context(self) -> BlockPersistContext {
        self.persist_context
    }

    #[inline]
    pub(super) const fn persist_options(
        self,
        observer_requires_artifacts: bool,
    ) -> NativePersistOptions {
        NativePersistOptions {
            capture_replay_artifacts: self.allows_replay_artifacts() && observer_requires_artifacts,
        }
    }

    #[inline]
    pub(super) const fn maintains_live_side_effects(self) -> bool {
        self.mode.maintains_live_side_effects()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn replay_artifacts_require_both_import_intent_and_an_observer() {
        let live = ImportPlan {
            mode: ImportMode::Live { verify: false },
            durability: ImportDurability::PerBlock,
            persist_context: BlockPersistContext::live(),
        };
        assert!(live.persist_options(true).capture_replay_artifacts);
        assert!(!live.persist_options(false).capture_replay_artifacts);

        let trusted = ImportPlan {
            mode: ImportMode::TrustedReplay { verify: false },
            durability: ImportDurability::DeferredBatch,
            persist_context: BlockPersistContext::trusted_replay(),
        };
        assert!(!trusted.persist_options(true).capture_replay_artifacts);
    }
}
