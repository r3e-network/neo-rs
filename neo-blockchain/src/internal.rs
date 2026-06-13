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

/// Result of [`classify_import_block`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportDisposition {
    /// The block's index is at or below the current persisted height.
    AlreadySeen,
    /// The block's index is exactly one above the current persisted height.
    NextExpected,
    /// The block's index is past the next expected height (a gap).
    FutureGap,
}

/// Classify an incoming import block relative to the current chain height.
pub(super) fn classify_import_block(current_height: u32, block_index: u32) -> ImportDisposition {
    if block_index <= current_height {
        ImportDisposition::AlreadySeen
    } else if block_index == current_height.saturating_add(1) {
        ImportDisposition::NextExpected
    } else {
        ImportDisposition::FutureGap
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
    /// Construct an empty FIFO list.
    pub(super) fn new() -> Self {
        Self::default()
    }

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
mod tests {
    use super::*;

    #[test]
    fn classify_already_seen_for_past_height() {
        assert_eq!(classify_import_block(10, 5), ImportDisposition::AlreadySeen);
        assert_eq!(
            classify_import_block(10, 10),
            ImportDisposition::AlreadySeen
        );
    }

    #[test]
    fn classify_next_expected_when_in_sequence() {
        assert_eq!(classify_import_block(7, 8), ImportDisposition::NextExpected);
    }

    #[test]
    fn classify_future_gap_for_skip() {
        assert_eq!(classify_import_block(3, 8), ImportDisposition::FutureGap);
    }

    #[test]
    fn schedule_idle_only_when_more_pending_without_backlog() {
        assert!(should_schedule_reverify_idle(true, false));
        assert!(!should_schedule_reverify_idle(false, false));
        assert!(!should_schedule_reverify_idle(true, true));
    }
}
