//! Internal blockchain-service helpers (not part of the public message API).
//!
//! These types and free functions are `pub(super)`-only machinery used by the
//! blockchain service; they are intentionally kept together rather than
//! split into vanity files. The public message types live in their own
//! per-type modules (`persist_completed`, `import`, `reverify`, `command`,
//! ...).

use std::collections::VecDeque;
use std::sync::Arc;

use neo_payloads::Block;

/// Result of [`ImportDisposition::classify_import_block`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportDisposition {
    /// The block's index is at or below the current persisted height.
    AlreadySeen,
    /// The block's index is exactly one above the current persisted height.
    NextExpected,
    /// The block's index is past the next expected height (a gap).
    FutureGap,
}

impl ImportDisposition {
    /// Classify an incoming import block relative to the current chain height.
    pub(super) fn classify_import_block(
        current_height: u32,
        block_index: u32,
    ) -> ImportDisposition {
        if block_index <= current_height {
            ImportDisposition::AlreadySeen
        } else if block_index == current_height.saturating_add(1) {
            ImportDisposition::NextExpected
        } else {
            ImportDisposition::FutureGap
        }
    }
}

/// One block parked because its parent has not landed yet.
#[derive(Debug, Clone)]
pub(super) struct UnverifiedBlock {
    pub(super) block: Arc<Block>,
    pub(super) relay: bool,
    pub(super) pre_verified: bool,
}

impl UnverifiedBlock {
    pub(super) fn new(block: Arc<Block>, relay: bool, pre_verified: bool) -> Self {
        Self {
            block,
            relay,
            pre_verified,
        }
    }
}

/// Per-index FIFO list of unverified blocks awaiting their parent to land.
#[derive(Debug, Clone, Default)]
pub struct UnverifiedBlocksList {
    blocks: VecDeque<UnverifiedBlock>,
}

impl UnverifiedBlocksList {
    pub(super) fn push_back(&mut self, block: UnverifiedBlock) {
        self.blocks.push_back(block);
    }

    pub(super) fn pop_front(&mut self) -> Option<UnverifiedBlock> {
        self.blocks.pop_front()
    }

    pub(super) fn len(&self) -> usize {
        self.blocks.len()
    }

    pub(super) fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

#[cfg(test)]
pub(super) fn should_schedule_reverify_idle(more_pending: bool, header_backlog: bool) -> bool {
    more_pending && !header_backlog
}

#[cfg(test)]
#[path = "tests/internal.rs"]
mod tests;
