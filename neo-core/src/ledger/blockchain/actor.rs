//
// actor.rs - Actor trait implementation for Blockchain
//

use super::*;

/// Interval at which the blockchain actor checks its unverified cache for
/// blocks that are ready to persist.  Without this timer, a stall can occur
/// when the TaskManager stops sending InventoryBlock messages (e.g. because
/// all blocks in the window are already in `received_block`) — the drain
/// would never run again.
const DRAIN_TIMER_INTERVAL: std::time::Duration = std::time::Duration::from_secs(5);

#[async_trait]
impl Actor for Blockchain {
    async fn pre_start(&mut self, ctx: &mut ActorContext) -> ActorResult {
        ctx.schedule_tell_repeatedly_cancelable(
            DRAIN_TIMER_INTERVAL,
            DRAIN_TIMER_INTERVAL,
            &ctx.self_ref(),
            BlockchainCommand::DrainUnverified,
            None,
        );
        Ok(())
    }

    async fn handle(
        &mut self,
        envelope: Box<dyn Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        if let Ok(message) = envelope.downcast::<BlockchainCommand>() {
            match *message {
                BlockchainCommand::PersistCompleted(persist) => {
                    self.handle_persist_completed(persist).await
                }
                BlockchainCommand::Import(import) => self.handle_import(import, ctx).await,
                BlockchainCommand::FillMemoryPool(fill) => {
                    self.handle_fill_memory_pool(fill, ctx).await
                }
                BlockchainCommand::Reverify(reverify) => self.handle_reverify(reverify, ctx).await,
                BlockchainCommand::InventoryBlock { block, relay, pre_verified } => {
                    self.handle_block_inventory(block, relay, pre_verified, ctx).await?
                }
                BlockchainCommand::InventoryExtensible { payload, relay } => {
                    self.handle_extensible_inventory(payload, relay, ctx)
                        .await?
                }
                BlockchainCommand::PreverifyCompleted(preverify) => {
                    self.handle_preverify_completed(preverify, ctx).await
                }
                BlockchainCommand::Headers(headers) => {
                    self.handle_headers(headers);
                }
                BlockchainCommand::Idle => self.handle_idle(ctx).await,
                BlockchainCommand::DrainUnverified => self.handle_drain_unverified(ctx).await,
                BlockchainCommand::FillCompleted => {}
                BlockchainCommand::RelayResult(result) => self.handle_relay_result(result).await,
                BlockchainCommand::Initialize => self.initialize().await,
                BlockchainCommand::AttachSystem(context) => self.system_context = Some(context),
            }
            Ok(())
        } else {
            Ok(())
        }
    }
}
