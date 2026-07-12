//! Live peer block-import composition.
//!
//! This adapter shares the node's staged-sync [`neo_runtime::BlockImportQueue`]
//! but retains `neo-blockchain` inventory semantics after preflight: dBFT
//! witness verification, future-block parking, ordered draining, durable batch
//! commits, events, and mempool maintenance remain owned by the canonical
//! blockchain service.

use std::sync::Arc;

use neo_blockchain::BlockchainHandle;
use neo_payloads::Block;
use neo_runtime::{BlockImportQueue, ServiceResult};
use tracing::{debug, warn};

/// Result of admitting one unsolicited peer block burst to the service loop.
///
/// `submitted` means stateless preflight succeeded and the candidate was sent
/// to the live inventory command. Canonical witness/state validation can still
/// reject or park it later inside `neo-blockchain`.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LiveBlockImportSummary {
    /// Number of candidates received from the inventory relay.
    pub received: usize,
    /// Number of preflight-accepted candidates submitted in original order.
    pub submitted: usize,
    /// Number of malformed candidates filtered before canonical import.
    pub rejected: usize,
}

/// Preflight adapter for unsolicited live P2P block inventory.
#[derive(Clone, Debug)]
pub struct LiveBlockImportPipeline {
    blockchain: BlockchainHandle,
    import_queue: Arc<BlockImportQueue<BlockchainHandle>>,
}

impl LiveBlockImportPipeline {
    /// Compose live inventory over the canonical service and its shared queue.
    #[must_use]
    pub fn new(
        blockchain: BlockchainHandle,
        import_queue: Arc<BlockImportQueue<BlockchainHandle>>,
    ) -> Self {
        Self {
            blockchain,
            import_queue,
        }
    }

    /// Returns the exact bounded queue shared with staged range sync.
    #[must_use]
    pub fn import_queue(&self) -> Arc<BlockImportQueue<BlockchainHandle>> {
        Arc::clone(&self.import_queue)
    }

    /// Filter one peer burst and submit every verified candidate without
    /// cloning its block allocation.
    ///
    /// Unlike staged sync, live inventory is intentionally lossy: one malformed
    /// candidate cannot poison otherwise valid blocks that happened to share a
    /// relay batch. Rejections are reported in aggregate and at debug detail;
    /// accepted candidates retain their original relative order.
    pub async fn submit_peer_blocks(
        &self,
        blocks: Vec<Arc<Block>>,
    ) -> ServiceResult<LiveBlockImportSummary> {
        let received = blocks.len();
        let checked = self.import_queue.check_blocks(blocks).await?;
        let submitted = checked.accepted_len();
        let rejected = checked.rejected_len();

        for rejection in checked.rejected() {
            debug!(
                target: "neo::sync",
                position = rejection.position(),
                category = rejection.error().category(),
                error = %rejection.error(),
                "peer block rejected by live import preflight"
            );
        }
        if rejected > 0 {
            warn!(
                target: "neo::sync",
                received,
                submitted,
                rejected,
                "filtered malformed peer blocks before live import"
            );
        }

        let summary = LiveBlockImportSummary {
            received,
            submitted,
            rejected,
        };
        if checked.is_empty() {
            return Ok(summary);
        }

        self.blockchain
            .submit_checked_inventory_blocks(checked, true)
            .await?;
        Ok(summary)
    }
}

#[cfg(test)]
#[path = "../../tests/composition/live_block_import_pipeline.rs"]
mod tests;
