//! Blockchain service command dispatch.
//!
//! The run-loop module owns scheduling; this module owns routing one
//! [`BlockchainCommand`] to the concrete handler that implements it. Keeping the
//! match in one place preserves Rust's compile-time exhaustiveness check for
//! command variants.

use super::BlockchainService;
use crate::command::BlockchainCommand;
use crate::service::MempoolLike;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Dispatch a single command to its handler. Public for testing
    /// — production callers go through [`Self::run`].
    pub async fn dispatch(&mut self, cmd: BlockchainCommand) {
        match cmd {
            BlockchainCommand::Import(import) => {
                // Import commands without a reply channel still produce a reply
                // containing error information. Log errors to avoid silently
                // discarding import failures.
                let reply = self.handle_import(import).await;
                if let Some(ref err) = reply.error {
                    tracing::warn!(
                        target: "neo",
                        error = %err,
                        imported = reply.imported,
                        "blockchain import completed with error"
                    );
                }
            }
            BlockchainCommand::ImportBlocks { import, reply } => {
                let result = self.handle_import(import).await;
                let _ = reply.send(result);
            }
            BlockchainCommand::FillCompleted => {}
            BlockchainCommand::Reverify(reverify) => {
                self.handle_reverify(reverify).await;
            }
            BlockchainCommand::ConsensusBlock { block, relay } => {
                if let Err(error) = self.handle_block_inventory(block, relay, true).await {
                    tracing::warn!(target: "neo", %error, "consensus block rejected");
                }
            }
            BlockchainCommand::CheckedInventoryBlocks { checked, relay } => {
                if let Err(error) = self
                    .handle_checked_block_inventory_batch(checked, relay)
                    .await
                {
                    tracing::error!(target: "neo", %error, "inventory block batch failed");
                }
            }
            BlockchainCommand::ImportBlock { block, reply } => {
                let before_height = self.ledger.current_height();
                let result = self.handle_block_inventory(block, false, false).await;
                let imported = self.ledger.current_height() > before_height;
                if let Err(error) = result {
                    tracing::warn!(target: "neo", %error, "import block rejected");
                }
                let _ = reply.send(imported);
            }
            BlockchainCommand::InventoryExtensible { payload, relay } => {
                let _ = self.handle_extensible_inventory(payload, relay).await;
            }
            BlockchainCommand::ValidateHeaders { headers, reply } => {
                let outcome = self.handle_headers(headers);
                let _ = reply.send(outcome);
            }
            BlockchainCommand::Idle => {
                self.handle_idle().await;
            }
            BlockchainCommand::DrainUnverified => {
                self.handle_drain_unverified().await;
            }
            BlockchainCommand::RelayResult(result) => {
                self.handle_relay_result(result).await;
            }
            BlockchainCommand::Initialize { reply } => {
                let _ = reply.send(self.initialize().await);
            }
            BlockchainCommand::Shutdown => {}
            BlockchainCommand::AddTransaction {
                transaction,
                origin,
                reply,
            } => {
                let _ = reply.send(self.add_transaction(origin, transaction).await);
            }
            BlockchainCommand::GetHeight { reply } => {
                let _ = reply.send(self.ledger.current_height());
            }
            BlockchainCommand::GetBlock { hash, reply } => {
                let block = self
                    .ledger
                    .get_block(&hash)
                    .or_else(|| self.full_block_from_store(&hash));
                let _ = reply.send(block);
            }
            BlockchainCommand::GetBlockByHeight { height, reply } => {
                let block = self.ledger.get_block_by_height(height).or_else(|| {
                    self.block_hash_from_store(height)
                        .and_then(|hash| self.full_block_from_store(&hash))
                });
                let _ = reply.send(block);
            }
        }
    }
}
