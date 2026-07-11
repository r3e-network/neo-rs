use std::sync::Arc;
use std::time::Instant;

use neo_payloads::Block;
use tracing::debug;

use crate::block_processing::BatchPersistResources;
use crate::command::ImportBlocksStats;
use crate::native_persist::NativePersistOptions;
use crate::service::{BlockchainService, MempoolLike};
use crate::service_context::BlockPersistContext;

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Persist one accepted import-command block through the normal C#-compatible path.
    ///
    /// C# `Blockchain.OnImport` runs `Persist(block)` - the state transition -
    /// before the block becomes the new tip. This helper owns that accepted-block
    /// sequence: native persistence, hot ledger cache insertion, live-store flush,
    /// committed hooks, mempool maintenance, header cleanup, and parked-child
    /// draining. The outer import loop remains responsible for ordering,
    /// verification, empty-block fast paths, and accepted-prefix accounting.
    pub(crate) async fn persist_import_block_for_command(
        &self,
        block: &Block,
        defer_store_commit: bool,
        persist_options: NativePersistOptions,
        persist_context: BlockPersistContext,
        batch_persist_resources: Option<&BatchPersistResources<S::NativeProvider, S::CacheBacking>>,
        stats: &mut ImportBlocksStats,
    ) -> Result<(), String> {
        let index = block.index();
        let transaction_block = !block.transactions.is_empty();
        let clone_start = transaction_block.then(Instant::now);
        let block = Arc::new(block.clone());
        if let Some(start) = clone_start {
            stats.transaction_block_clone_elapsed += start.elapsed();
        }
        let hash = Self::try_block_hash(block.as_ref())
            .map_err(|error| format!("import block {index} hash failed: {error}"))?;

        let transaction_block = !block.transactions.is_empty();
        let transaction_start = transaction_block.then(Instant::now);
        let persisted = if defer_store_commit {
            if let Some(resources) = batch_persist_resources {
                self.persist_block_sequence_with_resources(
                    Arc::clone(&block),
                    persist_options,
                    persist_context,
                    resources,
                )
            } else {
                self.persist_block_sequence_with_options(Arc::clone(&block), persist_options)
                    .await
            }
        } else {
            self.persist_block_sequence_with_options(Arc::clone(&block), persist_options)
                .await
        };
        if !persisted {
            return Err(format!(
                "import aborted at height {index}: native persistence pipeline failed"
            ));
        }
        if let Some(start) = transaction_start {
            stats.transaction_blocks += 1;
            stats.transaction_elapsed += start.elapsed();
        }

        // Ordinary imports flush each block immediately. A policy-approved
        // sync batch or trusted replay keeps staging into the shared snapshot
        // and flushes once after the accepted batch.
        if !defer_store_commit {
            self.system.commit_to_store().map_err(|error| {
                format!("import aborted at height {index}: durable store commit failed: {error}")
            })?;
        }

        let ledger_insert_start = transaction_block.then(Instant::now);
        self.ledger
            .insert_block_arc_with_hash(Arc::clone(&block), hash);
        if let Some(start) = ledger_insert_start {
            stats.transaction_ledger_insert_elapsed += start.elapsed();
        }

        if !defer_store_commit {
            let committed_hook_start = transaction_block.then(Instant::now);
            self.system
                .block_committed_with_context(block.as_ref(), persist_context);
            if let Some(start) = committed_hook_start {
                stats.transaction_committed_hook_elapsed += start.elapsed();
            }
        }

        // Deferred imports replay these effects only after batch durability.
        // Per-block imports retain the C# ordering here.
        if !defer_store_commit {
            self.mempool.block_persisted(block.as_ref());
            self.reverify_mempool_after_persist(
                index,
                self.system.settings().max_transactions_per_block as usize,
            );
            if !persist_context.is_trusted_replay() {
                self.event_tx
                    .send(crate::RuntimeEvent::Imported {
                        hash,
                        height: index,
                        timestamp: block.timestamp(),
                    })
                    .ok();
            }
            self.header_cache.remove_up_to(index);

            let drained = self.handle_drain_unverified_blocks().await;
            if drained > 0 {
                debug!(target: "neo", drained, "drained parked unverified blocks after import");
            }
        }

        Ok(())
    }
}
