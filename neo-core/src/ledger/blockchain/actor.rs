//
// actor.rs - Actor trait implementation for Blockchain
//

use super::*;

#[async_trait]
impl Actor for Blockchain {
    async fn pre_start(&mut self, _ctx: &mut ActorContext) -> ActorResult {
        Ok(())
    }

    async fn handle(
        &mut self,
        envelope: Box<dyn Any + Send>,
        ctx: &mut ActorContext,
    ) -> ActorResult {
        match envelope.downcast::<BlockchainCommand>() {
            Ok(message) => {
                match *message {
                    BlockchainCommand::PersistCompleted(persist) => {
                        self.handle_persist_completed(persist).await
                    }
                    BlockchainCommand::Import(import) => self.handle_import(import, ctx).await,
                    BlockchainCommand::FillMemoryPool(fill) => {
                        self.handle_fill_memory_pool(fill, ctx).await
                    }
                    BlockchainCommand::Reverify(reverify) => {
                        self.handle_reverify(reverify, ctx).await
                    }
                    BlockchainCommand::InventoryBlock { block, relay } => {
                        self.handle_block_inventory(block, relay, ctx).await?
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
                    BlockchainCommand::FillCompleted => {}
                    BlockchainCommand::RelayResult(result) => {
                        self.handle_relay_result(result).await
                    }
                    BlockchainCommand::Initialize => self.initialize().await,
                    BlockchainCommand::AttachSystem(context) => self.system_context = Some(context),
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }
}
