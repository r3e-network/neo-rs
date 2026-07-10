use std::sync::Arc;

use tracing::{debug, warn};

use crate::persist_completed::PersistCompleted;
use crate::service::{BlockchainService, MempoolLike};
use crate::service_context::BlockPersistContext;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Handle a [`BlockchainCommand::PersistCompleted`]: durably commit the
    /// store, then update hot ledger caches, evict persisted transactions, run
    /// post-commit observers, and broadcast the persistence event.
    pub(crate) async fn handle_persist_completed(&self, persist: PersistCompleted) {
        let PersistCompleted { block } = persist;
        let index = block.index();
        let hash = match Self::try_block_hash(block.as_ref()) {
            Ok(hash) => hash,
            Err(error) => {
                warn!(
                    target: "neo",
                    error = %error,
                    index,
                    "persist completed block hash computation failed"
                );
                return;
            }
        };
        debug!(
            target: "neo",
            index,
            tx_count = block.transactions.len(),
            "persist completed for block"
        );

        // Flush the persisted state through to the durable backing store
        // before publishing it through in-memory caches or observers.
        if let Err(error) = self.system.commit_to_store() {
            warn!(
                target: "neo",
                %error,
                index,
                "persist-completed durable store commit failed"
            );
            return;
        }

        self.ledger
            .insert_block_arc_with_hash(Arc::clone(&block), hash);

        for transaction in &block.transactions {
            if let Ok(hash) = transaction.try_hash() {
                self.ledger.remove_transaction(&hash);
            }
        }

        self.header_cache.remove_up_to(index);
        self.system
            .block_committed_with_context(block.as_ref(), BlockPersistContext::live());
        self.event_tx
            .send(crate::RuntimeEvent::Imported {
                hash,
                height: index,
                timestamp: block.header.timestamp(),
            })
            .ok();
    }
}
