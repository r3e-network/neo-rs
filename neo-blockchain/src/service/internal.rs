//! Internal blockchain-service helpers (not part of the public message API).
//!
//! These types and free functions are crate-private machinery used by the
//! blockchain service; they are intentionally kept together rather than
//! split into vanity files. The public message types live in their own
//! per-type modules (`import`, `reverify`, `command`, ...).

use std::collections::{BTreeMap, VecDeque};
use std::sync::Arc;

use neo_payloads::Block;

/// Result of `ImportDisposition::classify_import_block`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ImportDisposition {
    /// The block's index is at or below the current persisted height.
    AlreadySeen,
    /// The block's index is exactly one above the current persisted height.
    NextExpected,
    /// The block's index is past the next expected height (a gap).
    FutureGap,
}

/// Whether canonical stateless block-integrity checks still need to run.
///
/// `Checked` is created only when dispatch receives a checker-typed
/// `CheckedBlockBatch`; it is separate from consensus-witness trust because
/// peer blocks must always verify their dBFT witness against the previous tip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlockIntegrity {
    /// Run version, Merkle-root, and duplicate-transaction validation.
    Unchecked,
    /// The exact production [`crate::BlockchainHandle`] checker already ran.
    Checked,
}

impl BlockIntegrity {
    pub(crate) const fn requires_check(self) -> bool {
        matches!(self, Self::Unchecked)
    }
}

impl ImportDisposition {
    /// Classify an incoming import block relative to the current chain height.
    pub(crate) fn classify_import_block(
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
pub(crate) struct UnverifiedBlock {
    pub(crate) block: Arc<Block>,
    pub(crate) relay: bool,
    pub(crate) consensus_witness_verified: bool,
    pub(crate) integrity: BlockIntegrity,
}

impl UnverifiedBlock {
    pub(crate) fn new(
        block: Arc<Block>,
        relay: bool,
        consensus_witness_verified: bool,
        integrity: BlockIntegrity,
    ) -> Self {
        Self {
            block,
            relay,
            consensus_witness_verified,
            integrity,
        }
    }
}

/// Per-index FIFO list of unverified blocks awaiting their parent to land.
#[derive(Debug, Clone, Default)]
pub struct UnverifiedBlocksList {
    blocks: VecDeque<UnverifiedBlock>,
}

impl UnverifiedBlocksList {
    pub(crate) fn push_back(&mut self, block: UnverifiedBlock) {
        self.blocks.push_back(block);
    }

    pub(crate) fn pop_front(&mut self) -> Option<UnverifiedBlock> {
        self.blocks.pop_front()
    }

    pub(crate) fn pop_back(&mut self) -> Option<UnverifiedBlock> {
        self.blocks.pop_back()
    }

    pub(crate) fn len(&self) -> usize {
        self.blocks.len()
    }

    pub(crate) fn is_empty(&self) -> bool {
        self.blocks.is_empty()
    }
}

/// Bounded parked-block storage with exact block-count accounting.
///
/// Heights remain ordered so the service can drain the next canonical block
/// and evict the farthest-future candidates under pressure. The cached count
/// avoids rescanning every height bucket for each incoming block.
#[derive(Debug, Default)]
pub(crate) struct UnverifiedBlockCache {
    by_height: BTreeMap<u32, UnverifiedBlocksList>,
    len: usize,
}

impl UnverifiedBlockCache {
    pub(crate) const fn len(&self) -> usize {
        self.len
    }

    pub(crate) fn push(&mut self, block: UnverifiedBlock) {
        self.by_height
            .entry(block.block.index())
            .or_default()
            .push_back(block);
        self.len += 1;
    }

    pub(crate) fn pop_front(&mut self, height: u32) -> Option<UnverifiedBlock> {
        let (block, empty) = {
            let list = self.by_height.get_mut(&height)?;
            let block = list.pop_front();
            (block, list.is_empty())
        };
        if block.is_some() {
            self.len -= 1;
        }
        if empty {
            self.by_height.remove(&height);
        }
        block
    }

    /// Evict up to `count` blocks from the highest heights.
    pub(crate) fn evict_highest(&mut self, count: usize) -> usize {
        let target = count.min(self.len);
        let mut evicted = 0usize;

        while evicted < target {
            let Some(height) = self.by_height.last_key_value().map(|(height, _)| *height) else {
                break;
            };
            let empty = {
                let Some(list) = self.by_height.get_mut(&height) else {
                    break;
                };
                while evicted < target && list.pop_back().is_some() {
                    evicted += 1;
                }
                list.is_empty()
            };
            if empty {
                self.by_height.remove(&height);
            }
        }

        self.len -= evicted;
        evicted
    }

    pub(crate) fn remove_up_to(&mut self, up_to_height: u32) -> usize {
        let mut removed = 0usize;
        while self
            .by_height
            .first_key_value()
            .is_some_and(|(height, _)| *height <= up_to_height)
        {
            let Some((_, blocks)) = self.by_height.pop_first() else {
                break;
            };
            removed += blocks.len();
        }
        self.len -= removed;
        removed
    }
}

#[cfg(test)]
pub(super) fn should_schedule_reverify_idle(more_pending: bool, header_backlog: bool) -> bool {
    more_pending && !header_backlog
}

#[cfg(test)]
#[path = "../tests/service/internal.rs"]
mod tests;
