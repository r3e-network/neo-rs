//! Internal blockchain-actor helpers (not part of the public message API).
//!
//! These types and free functions are `pub(super)`-only machinery used by the
//! blockchain actor; they are intentionally kept together rather than split
//! into vanity files. The public message types live in their own per-type
//! modules (`persist_completed`, `import`, `reverify`, `command`, ...).

use super::*;
use std::sync::Arc;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub(super) struct UnverifiedBlocksList {
    pub(super) blocks: Vec<Arc<Block>>,
    nodes: HashSet<String>,
}

impl UnverifiedBlocksList {
    #[allow(dead_code)]
    pub(super) fn new() -> Self {
        Self {
            blocks: Vec::new(),
            nodes: HashSet::new(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ImportDisposition {
    AlreadySeen,
    NextExpected,
    FutureGap,
}

pub(super) fn classify_import_block(current_height: u32, block_index: u32) -> ImportDisposition {
    if block_index <= current_height {
        ImportDisposition::AlreadySeen
    } else if block_index == current_height.saturating_add(1) {
        ImportDisposition::NextExpected
    } else {
        ImportDisposition::FutureGap
    }
}

#[cfg(test)]
pub(super) fn should_schedule_reverify_idle(more_pending: bool, header_backlog: bool) -> bool {
    more_pending && !header_backlog
}

pub use crate::ledger::transaction_router::PreverifyCompleted;
