//! Block verification + persistence loop.
//!
//! Stage B keeps the block-processing pipeline as a thin
//! placeholder: the heavy logic (drain batches, header cache
//! eviction, state-root validation) is the focus of Stage C, when
//! the new service is wired up to the `BlockExecutor` trait from
//! `neo-runtime`. The placeholder exists so the public surface of
//! the crate is stable and the rest of the workspace can compile
//! against it.

use std::sync::Arc;

use neo_payloads::Block;
use neo_primitives::verify_result::VerifyResult;
use tracing::debug;

use crate::service::BlockchainService;

const DRAIN_BATCH_SIZE: usize = 50;
const MAX_BLOCK_CACHE_SIZE: usize = 20_000;
const MAX_UNVERIFIED_CACHE_SIZE: usize = 20_000;

impl BlockchainService {
    /// Verify and persist a block. The Stage B implementation
    /// returns `Succeed` unconditionally; the full validation
    /// pipeline is the focus of Stage C.
    pub(crate) async fn on_new_block(&self, _block: Arc<Block>, _verify: bool) -> VerifyResult {
        debug!(target: "neo", "block processing pipeline (stage B stub)");
        VerifyResult::Succeed
    }

    /// Persist a consecutive block sequence. Stage B is a no-op.
    pub(crate) async fn persist_block_sequence(&self, _block: Arc<Block>) -> bool {
        true
    }

    /// Drain the unverified block cache. Stage B is a no-op.
    pub(crate) async fn handle_drain_unverified_blocks(&self) -> usize {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn constants_have_expected_values() {
        // Sanity check: the stage-B drain batch size is much smaller
        // than the legacy actor's to keep memory pressure low.
        assert!(DRAIN_BATCH_SIZE > 0);
        assert!(MAX_BLOCK_CACHE_SIZE >= DRAIN_BATCH_SIZE);
        assert!(MAX_UNVERIFIED_CACHE_SIZE >= DRAIN_BATCH_SIZE);
    }
}
