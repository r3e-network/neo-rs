use std::sync::Arc;

use crate::inventory_payload::InventoryPayload;
use crate::reverify::Reverify;
use crate::service::{BlockchainService, MempoolLike};

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Handle a [`BlockchainCommand::Reverify`] request.
    ///
    /// C# reverification replays decoded inventory through the same block,
    /// transaction, and extensible-payload admission paths used by live peer
    /// messages. Raw inventory is intentionally ignored here because decoding is
    /// a network boundary concern, not a blockchain service concern.
    pub(crate) async fn handle_reverify(&self, reverify: Reverify) {
        for item in reverify.inventories {
            match item.payload {
                InventoryPayload::Block(block) => {
                    let _ = self
                        .handle_block_inventory(Arc::new(*block), false, false)
                        .await;
                }
                InventoryPayload::Transaction(tx) => {
                    let _ = self.on_new_transaction(&tx, None);
                }
                InventoryPayload::Extensible(payload) => {
                    let _ = self.handle_extensible_inventory(*payload, false).await;
                }
                InventoryPayload::Raw(_, _) => {}
            }
        }
    }
}
