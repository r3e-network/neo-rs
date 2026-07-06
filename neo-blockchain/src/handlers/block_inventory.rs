use std::sync::Arc;

use neo_error::{CoreError, CoreResult};
use neo_payloads::block::Block;
use tracing::{debug, warn};

use crate::service::{BlockchainService, MempoolLike};

impl<S, M> BlockchainService<S, M>
where
    S: crate::service_context::SystemContext,
    M: MempoolLike,
{
    /// Handle a [`BlockchainCommand::InventoryBlock`] command.
    pub(crate) async fn handle_block_inventory(
        &self,
        block: Arc<Block>,
        relay: bool,
        pre_verified: bool,
    ) -> CoreResult<()> {
        self.handle_block_inventory_without_drain(block, relay, pre_verified, false)
            .await?;
        let drained = self.handle_drain_unverified_blocks().await;
        if drained > 0 {
            debug!(target: "neo", drained, "drained parked unverified blocks");
        }
        Ok(())
    }

    /// Handle a contiguous burst of inventory blocks without requiring one
    /// command-channel round trip per block. Each block still goes through the
    /// normal inventory validation/persist path.
    pub(crate) async fn handle_block_inventory_batch(
        &self,
        blocks: Vec<Arc<Block>>,
        relay: bool,
        pre_verified: bool,
    ) -> usize {
        let mut imported = 0usize;
        let mut direct_imported = 0usize;
        for block in blocks {
            let before_height = self.ledger.current_height();
            match self
                .handle_block_inventory_without_drain(block, relay, pre_verified, true)
                .await
            {
                Ok(()) => {}
                Err(error) => {
                    warn!(target: "neo", %error, "inventory block rejected in batch");
                    continue;
                }
            }
            let current_height = self.ledger.current_height();
            if current_height > before_height {
                imported += 1;
                direct_imported += 1;
            }
        }
        if direct_imported > 0 {
            self.system.commit_to_store();
        }
        let drained = self.handle_drain_unverified_blocks().await;
        if drained > 0 {
            debug!(target: "neo", drained, "drained parked unverified blocks after inventory batch");
            imported += drained;
        }
        imported
    }

    async fn handle_block_inventory_without_drain(
        &self,
        block: Arc<Block>,
        relay: bool,
        pre_verified: bool,
        defer_store_commit: bool,
    ) -> CoreResult<()> {
        let index = block.index();
        let current_height = self.ledger.current_height();

        if index <= current_height {
            debug!(
                target: "neo",
                index,
                current_height,
                "inventory block already persisted"
            );
            return Ok(());
        }

        if index > current_height + 1 {
            let hash = Self::try_block_hash(block.as_ref())?;
            self.ensure_block_matches_cached_header(index, hash)?;
            debug!(
                target: "neo",
                index,
                current_height,
                "inventory block is ahead of the chain tip; parking"
            );
            self.park_unverified_block(block, relay, pre_verified);
            return Ok(());
        }

        self.persist_next_expected_block_with_commit_policy(
            block,
            relay,
            pre_verified,
            defer_store_commit,
        )
        .await
    }

    pub(crate) async fn persist_next_expected_block(
        &self,
        block: Arc<Block>,
        relay: bool,
        pre_verified: bool,
    ) -> CoreResult<()> {
        self.persist_next_expected_block_with_commit_policy(block, relay, pre_verified, false)
            .await
    }

    async fn persist_next_expected_block_with_commit_policy(
        &self,
        block: Arc<Block>,
        relay: bool,
        pre_verified: bool,
        defer_store_commit: bool,
    ) -> CoreResult<()> {
        let wall_start = std::time::Instant::now();
        let index = block.index();
        let hash = Self::try_block_hash(block.as_ref())?;
        let current_height = self.ledger.current_height();

        if let Some(stop_height) = self.stop_at_height {
            if index > stop_height {
                return Err(CoreError::other(format!(
                    "validation stop height {stop_height} reached; refusing block {index}"
                )));
            }
        }

        if index <= current_height {
            return Ok(());
        }

        let after_hash = wall_start.elapsed();
        if index != current_height + 1 {
            return Err(CoreError::other(format!(
                "block {index} is not the next expected height {}",
                current_height + 1
            )));
        }

        // C# Blockchain.OnNewBlock: when the header-first path has already
        // accepted a header for this height, the full block must be byte-for-byte
        // the body for that header (same unsigned-header hash). A competing block
        // with a valid witness but a different hash is invalid, not a fork choice.
        self.ensure_block_matches_cached_header(index, hash)?;

        // Stateless block-integrity pre-checks before persisting a peer-relayed
        // block (the structural half of C# `Block.Verify`): version, transaction
        // merkle root, and duplicate transaction hashes.
        if let Err(error) =
            crate::block_validation::BlockValidator::validate_import_integrity(block.as_ref())
        {
            return Err(CoreError::other(format!(
                "block {index} failed import-integrity validation: {error}"
            )));
        }

        // C# Header.Verify (Blockchain.OnNewBlock runs block.Verify before
        // Persist): a peer-relayed block must pass the structural header checks
        // and carry a consensus witness that satisfies the PREVIOUS block's
        // NextConsensus (the committee/validators multisig address). Locally
        // produced (pre-verified) blocks from the consensus driver skip this.
        //
        // Trusted offline imports can explicitly set `verify = false` through
        // the import path. Peer-relayed blocks do not get that shortcut: live
        // sync must use the same consensus-witness rule at height 1 and at
        // height 10 million.
        if !pre_verified {
            self.verify_consensus_witness_against_store(block.as_ref())?;
        }

        let after_verify = wall_start.elapsed();

        // C# Blockchain.OnNewBlock → Persist(block): the native-contract
        // state transition runs before the block becomes the new tip.
        if !self.persist_block_sequence(Arc::clone(&block)).await {
            return Err(CoreError::other(format!(
                "native persistence pipeline failed for block {index}"
            )));
        }

        let after_persist = wall_start.elapsed();

        if let Err(error) = self.ledger.insert_block_arc(Arc::clone(&block)) {
            return Err(CoreError::other(format!("ledger insert: {error}")));
        }

        if !defer_store_commit {
            // Flush the block's native-persist writes through to the durable store.
            // Per-block commit is memory-safe (no unbounded DataCache growth) and
            // with fast-sync store mode (WAL disabled) the RocksDB write is only
            // ~17us - negligible compared to the 0.5ms native-contract persist.
            self.system.commit_to_store();
        }
        let after_commit = wall_start.elapsed();
        self.system.block_committed(block.as_ref());

        // Per-block timing breakdown (debug-level). Shows where wall-clock
        // time goes: hash, verify (signature), persist (native contracts),
        // or commit (RocksDB write). Enable with RUST_LOG=neo::sync=debug.
        let total_us = neo_runtime::time::elapsed_us(after_commit);
        let verify_us = neo_runtime::time::elapsed_us(after_verify - after_hash);
        let persist_us = neo_runtime::time::elapsed_us(after_persist - after_verify);
        let commit_us = neo_runtime::time::elapsed_us(after_commit - after_persist);
        debug!(
            target: "neo::sync",
            index,
            hash_us = neo_runtime::time::elapsed_us(after_hash),
            verify_us,
            persist_us,
            commit_us,
            total_us,
            "block persist timing"
        );
        // Feed the sync metrics system for the Prometheus /metrics endpoint
        // and the rolling throughput window.
        neo_runtime::sync_metrics::record_block(
            index as u64,
            verify_us,
            persist_us,
            commit_us,
            total_us,
        );

        // C# Blockchain.Persist -> MemPool.UpdatePoolForBlockPersisted: drop the
        // block's transactions from the pool and evict pooled conflicts, so
        // mined txs are no longer served to peers or re-proposed by consensus.
        self.mempool.block_persisted(block.as_ref());
        self.reverify_mempool_after_persist(
            index,
            self.system.settings().max_transactions_per_block as usize,
        );

        self.event_tx
            .send(crate::RuntimeEvent::Imported {
                hash,
                height: index,
                timestamp: block.header.timestamp(),
            })
            .ok();
        self.header_cache.remove_up_to(index);

        let _ = relay; // relay broadcast is handled by the network service
        Ok(())
    }
}
