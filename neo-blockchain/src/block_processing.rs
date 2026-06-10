//! Block verification + persistence loop.
//!
//! Stage B keeps the block-processing pipeline as a thin
//! placeholder: the heavy logic (drain batches, header cache
//! eviction, state-root validation) is the focus of Stage C, when
//! the new service is wired up to the `BlockExecutor` trait from
//! `neo-runtime`. The placeholder exists so the public surface of
//! the crate is stable and the rest of the workspace can compile
//! against it.
//!
//! The native-contract half of C# `Blockchain.Persist` IS wired:
//! when the [`crate::service_context::SystemContext`] exposes a
//! store snapshot, [`BlockchainService::persist_block_sequence`]
//! runs [`crate::native_persist::persist_block_natives`] (genesis
//! initialization + `OnPersist` + `PostPersist` native hooks) over
//! it. Transaction execution and the store commit remain Stage C
//! work — see the `native_persist` module docs for the precise gap.

use std::sync::Arc;

use neo_payloads::Block;
use neo_primitives::verify_result::VerifyResult;
use tracing::{debug, error};

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

    /// Persist a consecutive block sequence: run the C#
    /// `Blockchain.Persist` pipeline (native OnPersist + ledger
    /// records, per-transaction Application execution, native
    /// PostPersist) when the system context exposes a store snapshot.
    /// The pipeline stages all writes in a child cache and commits
    /// them into the snapshot only when the whole sequence succeeds
    /// (see [`crate::native_persist`]). Without a store snapshot this
    /// remains the Stage B no-op.
    pub(crate) async fn persist_block_sequence(&self, block: Arc<Block>) -> bool {
        let Some(snapshot) = self.system.store_snapshot() else {
            debug!(
                target: "neo",
                index = block.index(),
                "persist_block_sequence: no store snapshot exposed (stage B stub)"
            );
            return true;
        };
        let settings = self.system.settings();
        match crate::native_persist::persist_block_natives(snapshot, block, settings.as_ref()) {
            Ok(outcome) => {
                debug!(
                    target: "neo",
                    initialized = ?outcome.initialized,
                    engines = outcome.application_executed.len(),
                    "block persistence pipeline completed"
                );
                true
            }
            Err(err) => {
                error!(target: "neo", %err, "block persistence pipeline failed");
                false
            }
        }
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
