//! Inventory handling (inv announcements, getdata, mempool, blocks) for `RemoteNode`.
use super::RemoteNode;
use crate::contains_transaction_type::ContainsTransactionType;
use crate::cryptography::BloomFilter;
use crate::ledger::blockchain::BlockchainCommand;
use crate::neo_io::Serializable;
use crate::network::p2p::messages::{NetworkMessage, ProtocolMessage};
use crate::network::p2p::payloads::get_block_by_index_payload::GetBlockByIndexPayload;
use crate::network::p2p::payloads::get_blocks_payload::GetBlocksPayload;
use crate::network::p2p::payloads::inv_payload::{InvPayload, MAX_HASHES_COUNT};
use crate::network::p2p::payloads::merkle_block_payload::MerkleBlockPayload;
use crate::network::p2p::payloads::transaction::{Transaction, MAX_TRANSACTION_SIZE};
use crate::network::p2p::payloads::InventoryType;
use crate::network::p2p::payloads::{block::Block, extensible_payload::ExtensiblePayload};
use crate::network::p2p::task_manager::TaskManagerCommand;
use crate::smart_contract::native::ledger_contract::LedgerContract;
use crate::UInt160;
use crate::UInt256;
use tracing::{trace, warn};

impl RemoteNode {
    pub(super) fn on_inv(&mut self, payload: &InvPayload, ctx: &mut crate::akka::ActorContext) {
        if payload.is_empty() {
            return;
        }

        // Validate inventory count to prevent DoS (matches C# MaxHashesCount)
        if payload.hashes.len() > MAX_HASHES_COUNT {
            warn!(
                target: "neo",
                endpoint = %self.endpoint,
                count = payload.hashes.len(),
                max = MAX_HASHES_COUNT,
                "inventory payload exceeds maximum hash count, ignoring"
            );
            return;
        }

        let now = std::time::Instant::now();
        let ledger_contract = LedgerContract::new();
        let mut hashes = Vec::with_capacity(payload.hashes.len());

        match payload.inventory_type {
            InventoryType::Block => {
                let store_cache = self.system.store_cache();
                for hash in payload.hashes.iter().copied() {
                    if self.should_skip_inventory(&hash) {
                        continue;
                    }
                    if ledger_contract.contains_block(&store_cache, &hash) {
                        continue;
                    }
                    hashes.push(hash);
                }
            }
            InventoryType::Transaction => {
                let store_cache = self.system.store_cache();
                for hash in payload.hashes.iter().copied() {
                    if self.should_skip_inventory(&hash) {
                        continue;
                    }
                    if ledger_contract
                        .contains_transaction(&store_cache, &hash)
                        .unwrap_or(false)
                    {
                        continue;
                    }
                    hashes.push(hash);
                }
            }
            _ => {
                for hash in payload.hashes.iter().copied() {
                    if self.should_skip_inventory(&hash) {
                        continue;
                    }
                    hashes.push(hash);
                }
            }
        }

        if hashes.is_empty() {
            return;
        }

        for hash in &hashes {
            self.pending_known_hashes.try_add(*hash, now);
        }

        let command = TaskManagerCommand::NewTasks {
            payload: InvPayload::new(payload.inventory_type, hashes),
        };
        if let Err(err) = self
            .system
            .task_manager
            .tell_from(command, Some(ctx.self_ref()))
        {
            warn!(
                target: "neo",
                error = %err,
                "failed to forward inventory announcement to task manager"
            );
        }
    }

    pub(super) fn notify_inventory_completed(
        &self,
        hash: UInt256,
        block: Option<Block>,
        block_index: Option<u32>,
        ctx: &crate::akka::ActorContext,
    ) {
        if let Err(err) = self.system.task_manager.tell_from(
            TaskManagerCommand::InventoryCompleted {
                hash,
                block: Box::new(block),
                block_index,
            },
            Some(ctx.self_ref()),
        ) {
            warn!(
                target: "neo",
                error = %err,
                "failed to notify task manager about inventory completion"
            );
        }
    }

    pub(super) async fn on_transaction(
        &mut self,
        transaction: Transaction,
        ctx: &mut crate::akka::ActorContext,
    ) -> crate::akka::ActorResult {
        // Validate transaction size before processing (matches C# MAX_TRANSACTION_SIZE)
        let tx_size = transaction.size();
        if tx_size > MAX_TRANSACTION_SIZE {
            warn!(
                target: "neo",
                endpoint = %self.endpoint,
                tx_size,
                max_size = MAX_TRANSACTION_SIZE,
                "transaction exceeds maximum size, rejecting"
            );
            return Ok(());
        }

        let hash = transaction.hash();
        if !self.known_hashes.try_add(hash) {
            return Ok(());
        }
        self.pending_known_hashes.remove(&hash);
        self.notify_inventory_completed(hash, None, None, ctx);

        let contains = self.system.contains_transaction(&hash);
        let signer_accounts: Vec<UInt160> = transaction
            .signers()
            .iter()
            .map(|signer| signer.account)
            .collect();
        let has_conflict = !signer_accounts.is_empty()
            && self.system.contains_conflict_hash(&hash, &signer_accounts);

        if contains != ContainsTransactionType::NotExist || has_conflict {
            trace!(
                target: "neo",
                hash = %hash,
                contains = ?contains,
                has_conflict,
                "transaction skipped because it is already known or conflicts on-chain"
            );
            return Ok(());
        }

        if let Err(err) = self.system.tx_router.tell_from(
            crate::neo_system::TransactionRouterMessage::Preverify {
                transaction: transaction.clone(),
                relay: true,
            },
            Some(ctx.self_ref()),
        ) {
            warn!(
                target: "neo",
                %hash,
                error = %err,
                "failed to enqueue transaction for preverification"
            );
        }

        Ok(())
    }

    pub(super) async fn on_block(
        &mut self,
        mut block: Block,
        ctx: &mut crate::akka::ActorContext,
    ) -> crate::akka::ActorResult {
        let hash = block.hash();
        if !self.known_hashes.try_add(hash) {
            return Ok(());
        }
        self.pending_known_hashes.remove(&hash);
        self.last_block_index = block.index();
        let block_clone = block.clone();
        self.notify_inventory_completed(hash, Some(block_clone), Some(block.index()), ctx);
        let current_height = self.system.current_block_index();
        if block.index() > current_height.saturating_add(MAX_HASHES_COUNT as u32) {
            return Ok(());
        }
        trace!(
            target: "neo",
            index = block.index(),
            hash = %hash,
            "block received from remote node"
        );

        if let Err(err) = self.system.blockchain.tell_from(
            BlockchainCommand::InventoryBlock { block, relay: true },
            Some(ctx.self_ref()),
        ) {
            warn!(
                target: "neo",
                hash = %hash,
                error = %err,
                "failed to forward block to blockchain actor"
            );
        }
        Ok(())
    }

    pub(super) async fn on_extensible(
        &mut self,
        mut payload: ExtensiblePayload,
        ctx: &mut crate::akka::ActorContext,
    ) -> crate::akka::ActorResult {
        let hash = payload.hash();
        if !self.known_hashes.try_add(hash) {
            return Ok(());
        }
        self.pending_known_hashes.remove(&hash);
        self.notify_inventory_completed(hash, None, None, ctx);
        trace!(target: "neo", hash = %hash, "extensible payload received");
        if let Err(err) = self.system.blockchain.tell_from(
            BlockchainCommand::InventoryExtensible {
                payload,
                relay: true,
            },
            Some(ctx.self_ref()),
        ) {
            warn!(
                target: "neo",
                hash = %hash,
                error = %err,
                "failed to forward extensible payload to blockchain actor"
            );
        }
        Ok(())
    }

    pub(super) async fn on_mempool(&mut self) -> crate::akka::ActorResult {
        let hashes = self.system.mempool_transaction_hashes();
        if hashes.is_empty() {
            return Ok(());
        }

        for group in InvPayload::create_group(InventoryType::Transaction, hashes) {
            self.enqueue_message(NetworkMessage::new(ProtocolMessage::Inv(group)))
                .await?;
        }

        Ok(())
    }

    pub(super) async fn on_get_data(&mut self, payload: &InvPayload) -> crate::akka::ActorResult {
        if payload.is_empty() {
            return Ok(());
        }

        let mut not_found = Vec::with_capacity(payload.hashes.len());

        match payload.inventory_type {
            InventoryType::Transaction => {
                for hash in payload.hashes.iter().copied() {
                    if !self.sent_hashes.try_add(hash) {
                        continue;
                    }
                    if let Some(transaction) = self.system.try_get_transaction_from_mempool(&hash) {
                        self.enqueue_message(NetworkMessage::new(ProtocolMessage::Transaction(
                            transaction,
                        )))
                        .await?;
                    } else {
                        not_found.push(hash);
                    }
                }
            }
            InventoryType::Block => {
                for hash in payload.hashes.iter().copied() {
                    if !self.sent_hashes.try_add(hash) {
                        continue;
                    }
                    if let Some(mut block) = self.system.try_get_block(&hash) {
                        if let Some(flags) = self.bloom_filter_flags(&block) {
                            let payload = MerkleBlockPayload::create(&mut block, flags);
                            self.enqueue_message(NetworkMessage::new(
                                ProtocolMessage::MerkleBlock(payload),
                            ))
                            .await?;
                        } else {
                            self.enqueue_message(NetworkMessage::new(ProtocolMessage::Block(
                                block,
                            )))
                            .await?;
                        }
                    } else {
                        not_found.push(hash);
                    }
                }
            }
            InventoryType::Consensus | InventoryType::Extensible => {
                for hash in payload.hashes.iter().copied() {
                    if !self.sent_hashes.try_add(hash) {
                        continue;
                    }
                    if let Some(extensible) = self.system.try_get_relay_extensible(&hash) {
                        self.enqueue_message(NetworkMessage::new(ProtocolMessage::Extensible(
                            extensible,
                        )))
                        .await?;
                    } else if let Some(extensible) = self.system.try_get_extensible(&hash) {
                        self.enqueue_message(NetworkMessage::new(ProtocolMessage::Extensible(
                            extensible,
                        )))
                        .await?;
                    }
                }
            }
        }

        if !not_found.is_empty() {
            for group in InvPayload::create_group(payload.inventory_type, not_found) {
                self.enqueue_message(NetworkMessage::new(ProtocolMessage::NotFound(group)))
                    .await?;
            }
        }

        Ok(())
    }

    pub(super) async fn on_get_blocks(
        &mut self,
        payload: GetBlocksPayload,
    ) -> crate::akka::ActorResult {
        // Validate that the start hash exists in the ledger (matches C# behavior)
        if self.system.try_get_block(&payload.hash_start).is_none() {
            return Ok(());
        }

        let count = Self::normalize_request(payload.count, MAX_HASHES_COUNT);
        let hashes = self.system.block_hashes_from(&payload.hash_start, count);

        if hashes.is_empty() {
            return Ok(());
        }

        for group in InvPayload::create_group(InventoryType::Block, hashes) {
            self.enqueue_message(NetworkMessage::new(ProtocolMessage::Inv(group)))
                .await?;
        }

        Ok(())
    }

    pub(super) async fn on_get_block_by_index(
        &mut self,
        payload: GetBlockByIndexPayload,
    ) -> crate::akka::ActorResult {
        let count = Self::normalize_request(payload.count, MAX_HASHES_COUNT);
        if count == 0 {
            return Ok(());
        }

        for offset in 0..count {
            let index = payload.index_start.saturating_add(offset as u32);

            let Some(hash) = self.system.block_hash_at(index) else {
                break;
            };

            let Some(mut block) = self.system.try_get_block(&hash) else {
                break;
            };

            if let Some(flags) = self.bloom_filter_flags(&block) {
                let payload = MerkleBlockPayload::create(&mut block, flags);
                self.enqueue_message(NetworkMessage::new(ProtocolMessage::MerkleBlock(payload)))
                    .await?;
            } else {
                self.enqueue_message(NetworkMessage::new(ProtocolMessage::Block(block)))
                    .await?;
            }
        }

        Ok(())
    }

    pub(super) fn on_not_found(&mut self, payload: InvPayload, ctx: &crate::akka::ActorContext) {
        for hash in &payload.hashes {
            self.pending_known_hashes.remove(hash);
        }

        if let Err(err) = self.system.task_manager.tell_from(
            TaskManagerCommand::RestartTasks { payload },
            Some(ctx.self_ref()),
        ) {
            warn!(
                target: "neo",
                error = %err,
                "failed to notify task manager about inventory restart"
            );
        }
    }

    pub(super) fn bloom_filter_flags(&self, block: &Block) -> Option<Vec<bool>> {
        let filter = self.bloom_filter.as_ref()?;
        Some(
            block
                .transactions
                .iter()
                .map(|tx| Self::filter_matches_transaction(filter, tx))
                .collect(),
        )
    }

    fn filter_matches_transaction(filter: &BloomFilter, tx: &Transaction) -> bool {
        let hash_bytes = tx.hash().to_array();
        if filter.check(&hash_bytes) {
            return true;
        }

        tx.signers().iter().any(|signer| {
            let account_bytes = signer.account.as_bytes();
            filter.check(account_bytes.as_ref())
        })
    }
}
