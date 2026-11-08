//
// handlers.rs - Message handlers for Blockchain actor
//

use super::*;
use neo_protocol::execution::BlockHeader;
use neo_ledger::HeaderCache;

/// Verifies a block header against the header cache or store.
///
/// This function implements the complete header verification logic used during
/// block header synchronization. It validates:
/// - Self-consistency (primary index within bounds)
/// - Continuity (correct index, prev_hash, and timestamp against the previous header)
/// - Witness verification (cryptographic signature validation against next_consensus)
///
/// The verification first attempts to use the cached previous header for efficiency.
/// If the header cache is empty, it falls back to full verification against the store.
///
/// # Reference
/// C# implementation: neo/Blockchain.cs `Header.Verify()` delegation path
///
/// # Arguments
/// * `header` - The execution block header to verify
/// * `settings` - Protocol settings (validator count, timing rules)
/// * `store_cache` - Store snapshot for reading previous blocks
/// * `header_cache` - Header cache holding recently synchronized headers
///
/// # Returns
/// `true` if the header passes all validation checks, `false` otherwise.
fn verify_header_with_cache(
    header: &BlockHeader,
    settings: &ProtocolSettings,
    store_cache: &StoreCache,
    header_cache: &HeaderCache,
) -> bool {
    // Convert execution BlockHeader to payload Header for neo-ledger validation
    let payload_header = neo_protocol::execution::conversions::convert_ledger_header(header.clone());

    // Use neo_ledger's comprehensive header verification with cache support
    // This handles:
    // 1. Self-validation (primary index bounds check)
    // 2. Validation against previous header (index, prev_hash, timestamp progression)
    // 3. Witness verification (cryptographic signature against next_consensus script hash)
    // 4. Automatic fallback to store-based verification when cache is empty
    neo_ledger::verify_header_with_cache(
        &payload_header,
        settings,
        store_cache,
        header_cache,
    )
}

impl Blockchain {
    fn has_header_backlog(&self, context: &NeoSystemContext) -> bool {
        context.header_cache().count() > 0 || self.ledger.has_future_headers()
    }

    fn find_tip_in_store(
        ledger_contract: &LedgerContract,
        store_cache: &StoreCache,
    ) -> Option<(u32, UInt256)> {
        ledger_contract
            .get_block_hash_by_index(store_cache, 1)
            .ok()
            .flatten()?;

        let mut low = 1u32;
        let mut high = 1u32;

        loop {
            let next = high.saturating_mul(2);
            if next == high {
                break;
            }

            if ledger_contract
                .get_block_hash_by_index(store_cache, next)
                .ok()
                .flatten()
                .is_some()
            {
                low = next;
                high = next;
            } else {
                high = next;
                break;
            }
        }

        if high == low {
            let hash = ledger_contract
                .get_block_hash_by_index(store_cache, low)
                .ok()
                .flatten()?;
            return Some((low, hash));
        }

        let mut left = low;
        let mut right = high.saturating_sub(1);
        while left < right {
            let mid = left + (right - left).div_ceil(2);
            if ledger_contract
                .get_block_hash_by_index(store_cache, mid)
                .ok()
                .flatten()
                .is_some()
            {
                left = mid;
            } else {
                right = mid.saturating_sub(1);
            }
        }

        let hash = ledger_contract
            .get_block_hash_by_index(store_cache, left)
            .ok()
            .flatten()?;
        Some((left, hash))
    }

    pub(super) async fn handle_persist_completed(&self, persist: PersistCompleted) {
        let PersistCompleted { block } = persist;
        let index = block.index();
        let hash = match Self::try_block_hash(block.as_ref()) {
            Ok(hash) => hash,
            Err(error) => {
                tracing::warn!(
                    target: "neo",
                    error = %error,
                    index,
                    "persist completed block hash computation failed"
                );
                return;
            }
        };
        let tx_count = block.transactions.len();
        tracing::debug!(
            target: "neo",
            %hash,
            index,
            tx_count,
            "persist completed for block"
        );

        {
            self._block_cache.insert(hash, Arc::clone(&block));

            let prev_hash = *block.prev_hash();
            if !prev_hash.is_zero() {
                self._block_cache.remove(&prev_hash);
            }
        }

        if let Err(error) = self.ledger.insert_block(block.to_payload_block()) {
            tracing::warn!(
                target: "neo",
                %error,
                index,
                "failed to insert persisted block into ledger cache"
            );
        }

        for transaction in &block.transactions {
            let tx_hash = match transaction.try_hash() {
                Ok(hash) => hash,
                Err(error) => {
                    tracing::warn!(
                        target: "neo",
                        error = %error,
                        "transaction hash computation failed while clearing ledger cache after persist"
                    );
                    continue;
                }
            };
            self.ledger.remove_transaction(&tx_hash);
        }

        if let Some(context) = &self.system_context {
            let store_cache = context.store_cache();
            let snapshot = store_cache.data_cache();
            let settings = context.settings();
            let header_backlog_present = self.has_header_backlog(context);
            let payload_block = block.to_payload_block();
            context
                .memory_pool()
                .lock()
                .update_pool_for_block_persisted(
                    &payload_block,
                    snapshot,
                    settings.as_ref(),
                    header_backlog_present,
                );
        }

        if let Some(context) = &self.system_context {
            // TaskManager depends on persist notifications to release per-peer
            // block queues and request subsequent ranges.
            context
                .actor_system
                .event_stream()
                .publish(PersistCompleted {
                    block: Arc::clone(&block),
                });
        }

        self.remove_unverified_index(index);

        if let Some(context) = &self.system_context {
            context.header_cache().remove_up_to(index);
        }

        self._extensible_witness_white_list.write().await.clear();
    }

    pub(super) fn handle_headers(&self, headers: Vec<Arc<Block>>) {
        if headers.is_empty() {
            return;
        }

        let Some(context) = &self.system_context else {
            return;
        };

        let header_cache = context.header_cache();
        let store_cache = context.store_cache();
        let settings = context.settings();
        let current_height = context.ledger().current_height();
        let mut header_height = header_cache
            .last()
            .map(|header| header.index())
            .unwrap_or(current_height);

        for block in headers.into_iter() {
            let header = block.to_payload_block().header;
            let index = header.index();
            if index <= header_height {
                continue;
            }

            if index != header_height + 1 {
                break;
            }

            let block_header: BlockHeader = header.clone().into();
            if !verify_header_with_cache(&block_header, settings.as_ref(), &store_cache, &header_cache) {
                break;
            }

            if !header_cache.add(header.clone()) {
                break;
            }

            header_height = index;
        }
    }

    pub(super) async fn handle_import(&self, import: Import, ctx: &ActorContext) {
        let Some(context) = &self.system_context else {
            tracing::debug!(target: "neo", "import requested before system context attached");
            if let Some(sender) = ctx.sender() {
                // Use the first block from the import request as the completed block
                let block = import.block.clone();
                let _ = sender.tell(ImportCompleted { block });
            }
            return;
        };

        let settings = context.settings();
        let store_cache = context.store_cache();
        let ledger_contract = LedgerContract::new();
        let mut current_height = ledger_contract
            .current_index(&store_cache)
            .unwrap_or_else(|_| context.ledger().current_height());

        let mut last_block = import.block.clone();
        for block in import.blocks {
            let index = block.index();
            match classify_import_block(current_height, index) {
                ImportDisposition::AlreadySeen => continue,
                ImportDisposition::FutureGap => {
                    tracing::warn!(
                        target: "neo",
                        expected = current_height + 1,
                        actual = index,
                        "import block out of sequence"
                    );
                    break;
                }
                ImportDisposition::NextExpected => {}
            }

            if import.verify {
                let payload_block = block.to_payload_block();
                if !neo_ledger::verify_block(&payload_block, settings.as_ref(), &store_cache) {
                    tracing::warn!(
                        target: "neo",
                        height = index,
                        "import block failed verification"
                    );
                    break;
                }
            }

            last_block = block.clone();
            if !self.persist_block_via_system(&block) {
                tracing::warn!(
                    target: "neo",
                    height = index,
                    "stopping import after block persist failure"
                );
                break;
            }
            self.handle_persist_completed(PersistCompleted {
                block: block.clone(),
            })
            .await;
            current_height = index;
        }

        if let Some(sender) = ctx.sender() {
            let _ = sender.tell(ImportCompleted { block: last_block });
        }
    }

    pub(super) async fn handle_fill_memory_pool(&self, fill: FillMemoryPool, ctx: &ActorContext) {
        if let Some(context) = &self.system_context {
            let store_cache = context.store_cache();
            let settings = context.settings();
            let memory_pool = context.memory_pool();
            let mut pool = memory_pool.lock();
            pool.invalidate_all_transactions();
            let snapshot = store_cache.data_cache();
            let max_traceable_blocks = LedgerContract::new()
                .max_traceable_blocks_snapshot(&store_cache, &settings)
                .unwrap_or(settings.max_traceable_blocks);
            for tx in fill.transactions {
                if self.transaction_exists_on_chain(&tx, &store_cache) {
                    continue;
                }

                if self.conflict_exists_on_chain(&tx, &store_cache, max_traceable_blocks) {
                    continue;
                }

                let tx_hash = match tx.try_hash() {
                    Ok(hash) => hash,
                    Err(error) => {
                        tracing::warn!(
                            target: "neo",
                            error = %error,
                            "transaction hash computation failed while filling memory pool"
                        );
                        continue;
                    }
                };
                let _ = pool.remove_unverified(&tx_hash);

                let _ = pool.try_add(tx, snapshot, &settings);
            }

            let needs_idle = pool.unverified_count() > 0;
            drop(pool);

            if needs_idle && let Err(error) = ctx.self_ref().tell(BlockchainCommand::Idle) {
                tracing::debug!(
                    target: "neo",
                    %error,
                    "failed to enqueue idle reverify after filling memory pool"
                );
            }

            if let Some(sender) = ctx.sender() {
                let _ = sender.tell(FillCompleted);
            }
        }
    }

    pub(super) async fn handle_reverify(&self, reverify: Reverify, ctx: &ActorContext) {
        for item in reverify.inventories {
            match item.payload {
                InventoryPayload::Raw(inventory_type, payload) => {
                    let cache_key = InventoryCacheKey::new(inventory_type, &payload);
                    if let Some(cached) = self.inventory_cache_get(&cache_key).await {
                        self.handle_reverify_payload(cached, ctx).await;
                        continue;
                    }

                    let decoded = match inventory_type {
                        InventoryType::Block => {
                            Self::deserialize_inventory::<neo_protocol::payloads::Block>(&payload)
                                .map(|payload_block| {
                                    let exec_block = Block::from(&payload_block);
                                    InventoryPayload::Block(Arc::new(exec_block))
                                })
                        }
                        InventoryType::Transaction => {
                            Self::deserialize_inventory::<Transaction>(&payload)
                                .map(|tx| InventoryPayload::Transaction(tx))
                        }
                        InventoryType::Consensus | InventoryType::Extensible => {
                            Self::deserialize_inventory::<ExtensiblePayload>(&payload)
                                .map(|payload| InventoryPayload::Extensible(payload))
                        }
                    };

                    if let Some(payload) = decoded {
                        self.inventory_cache_insert(cache_key, payload.clone())
                            .await;
                        self.handle_reverify_payload(payload, ctx).await;
                    } else {
                        tracing::debug!(
                            target: "neo",
                            inventory_type = ?inventory_type,
                            "failed to deserialize payload during reverify"
                        );
                    }
                }
                payload => {
                    self.handle_reverify_payload(payload, ctx).await;
                }
            };
        }
    }

    async fn handle_reverify_payload(&self, payload: InventoryPayload, ctx: &ActorContext) {
        match payload {
            InventoryPayload::Block(block) => {
                if let Err(error) = self
                    .handle_block_inventory(block, false, false, ctx)
                    .await
                {
                    tracing::debug!(
                        target: "neo",
                        %error,
                        "failed to reverify block inventory"
                    );
                }
            }
            InventoryPayload::Transaction(tx) => {
                let _ = self.on_new_transaction(&tx);
            }
            InventoryPayload::Extensible(payload) => {
                if let Err(error) = self.handle_extensible_inventory(payload, false, ctx).await {
                    tracing::debug!(
                        target: "neo",
                        %error,
                        "failed to reverify extensible payload"
                    );
                }
            }
            InventoryPayload::Raw(_, _) => {}
        }
    }

    pub(super) async fn handle_block_inventory(
        &self,
        block: Arc<Block>,
        relay: bool,
        _pre_verified: bool,
        ctx: &ActorContext,
    ) -> ActorResult {
        let index = block.index();
        let hash = match Self::try_block_hash(block.as_ref()) {
            Ok(hash) => hash,
            Err(error) => {
                tracing::warn!(
                    target: "neo",
                    error = %error,
                    index,
                    "block hash computation failed, rejecting inventory"
                );
                return Ok(());
            }
        };

        let result = self.on_new_block(Arc::clone(&block), true).await;

        // Opportunistically drain unverified cache inline. Scheduling via
        // self_ref().tell() can silently fail when the mailbox is full (try_send),
        // so we call handle_drain_unverified directly to ensure the next-to-persist
        // block gets processed immediately.
        if let Some(context) = &self.system_context {
            let current_height = context.ledger().current_height();
            let next_index = current_height.saturating_add(1);
            if self._block_cache_unverified.contains_key(&next_index) {
                self.handle_drain_unverified(ctx).await;
            }
        }

        if let Some(context) = &self.system_context {
            let inventory = if relay && result == VerifyResult::Succeed {
                Some(RelayInventory::Block(block.to_payload_block()))
            } else {
                None
            };

            self.publish_inventory_relay_result(
                context,
                hash,
                InventoryType::Block,
                Some(index),
                result,
                relay,
                inventory,
                ctx,
            );
        }

        Ok(())
    }

    pub(super) async fn handle_extensible_inventory(
        &self,
        mut payload: ExtensiblePayload,
        relay: bool,
        ctx: &ActorContext,
    ) -> ActorResult {
        let hash = match payload.try_hash() {
            Ok(hash) => hash,
            Err(error) => {
                tracing::warn!(
                    target: "neo",
                    error = %error,
                    "extensible payload hash computation failed, rejecting inventory"
                );
                return Ok(());
            }
        };

        let result = self.on_new_extensible(payload.clone()).await;

        if let Some(context) = &self.system_context {
            let inventory = if relay && result == VerifyResult::Succeed {
                Some(RelayInventory::Extensible(payload.clone()))
            } else {
                None
            };

            self.publish_inventory_relay_result(
                context,
                hash,
                InventoryType::Extensible,
                None,
                result,
                relay,
                inventory,
                ctx,
            );
        }

        Ok(())
    }

    pub(super) async fn handle_idle(&self, ctx: &ActorContext) {
        if let Some(system_context) = &self.system_context {
            let store_cache = system_context.store_cache();
            let settings = system_context.settings();
            let snapshot = store_cache.data_cache();
            let header_backlog = self.has_header_backlog(system_context);
            let more_pending = system_context
                .memory_pool()
                .lock()
                .reverify_top_unverified_transactions(
                    MAX_TX_TO_REVERIFY_PER_IDLE,
                    snapshot,
                    &settings,
                    header_backlog,
                );

            if more_pending && let Err(error) = ctx.self_ref().tell(BlockchainCommand::Idle) {
                tracing::debug!(
                    target: "neo",
                    %error,
                    "failed to enqueue idle reverify continuation"
                );
            }
        }
    }

    pub(super) async fn handle_preverify_completed(
        &self,
        task: PreverifyCompleted,
        ctx: &ActorContext,
    ) {
        let Some(context) = &self.system_context else {
            tracing::debug!(
                target: "neo",
                "preverify completed before system context attached; ignoring"
            );
            return;
        };

        let result = if task.result == VerifyResult::Succeed {
            self.on_new_transaction(&task.transaction)
        } else {
            task.result
        };

        let tx_hash = match task.transaction.try_hash() {
            Ok(hash) => hash,
            Err(error) => {
                tracing::warn!(
                    target: "neo",
                    error = %error,
                    "transaction hash computation failed after preverification"
                );
                return;
            }
        };

        self.publish_inventory_relay_result(
            context,
            tx_hash,
            InventoryType::Transaction,
            None,
            result,
            task.relay,
            if task.relay && result == VerifyResult::Succeed {
                Some(RelayInventory::Transaction(task.transaction.clone()))
            } else {
                None
            },
            ctx,
        );
    }

    #[allow(clippy::too_many_arguments)]
    fn publish_inventory_relay_result(
        &self,
        context: &Arc<NeoSystemContext>,
        hash: UInt256,
        inventory_type: InventoryType,
        block_index: Option<u32>,
        result: VerifyResult,
        relay: bool,
        inventory: Option<RelayInventory>,
        ctx: &ActorContext,
    ) {
        if relay
            && result == VerifyResult::Succeed
            && let Some(inv) = inventory
            && let Err(error) = context.local_node.relay_directly(inv, block_index)
        {
            tracing::debug!(
                target: "neo",
                %error,
                "failed to record relay broadcast"
            );
        }

        // Publish relay result event - RelayResult is Result<T, RelayError>, not a struct
        // TODO: Define proper relay event type or use existing event system
        // For now, we skip publishing the relay result to fix compilation
        // context
        //     .actor_system
        //     .event_stream()
        //     .publish(relay_message.clone());

        if result == VerifyResult::Succeed && matches!(inventory_type, InventoryType::Transaction) {
            context.broadcast_plugin_event(PluginEvent::TransactionReceived {
                tx_hash: hash.to_string(),
            });
        }

        // TODO: Restore relay_message when proper message types are available
        // if let Some(sender) = ctx.sender()
        //     && let Err(error) = sender.tell(relay_message)
        // {
        //     tracing::debug!(
        //         target: "neo",
        //         %error,
        //         "failed to reply with relay result to sender"
        //     );
        // }
    }

    pub(super) async fn handle_relay_result(&self, _result: neo_ledger::RelayResult<()>) {}

    pub(super) async fn initialize(&self) {
        let Some(context) = &self.system_context else {
            tracing::debug!(target: "neo", "blockchain initialize requested before context attached");
            return;
        };

        let store_cache = context.store_cache();
        let ledger_contract = LedgerContract::new();
        let persisted_index = ledger_contract.current_index(&store_cache).unwrap_or(0);
        let persisted_hash = ledger_contract
            .current_hash(&store_cache)
            .unwrap_or_else(|_| UInt256::zero());
        let has_block_one = ledger_contract
            .get_block_hash_by_index(&store_cache, 1)
            .ok()
            .flatten()
            .is_some();

        if persisted_index == 0 && has_block_one {
            if let Some((tip_index, tip_hash)) =
                Self::find_tip_in_store(&ledger_contract, &store_cache)
            {
                let snapshot = store_cache.data_cache();
                if let Err(error) =
                    ledger_contract.set_current_block_state(snapshot, &tip_hash, tip_index)
                {
                    tracing::warn!(
                        target: "neo",
                        %error,
                        tip_index,
                        "failed to repair persisted current block pointer"
                    );
                } else {
                    tracing::warn!(
                        target: "neo",
                        repaired_index = tip_index,
                        repaired_hash = %tip_hash,
                        "repaired stale current block pointer from storage"
                    );
                }
                return;
            }

            tracing::warn!(
                target: "neo",
                "detected historical blocks in storage but could not locate tip; skipping genesis re-initialization"
            );
            return;
        }

        // Genesis block is already persisted by NeoSystem::new before the
        // Blockchain actor receives the Initialize command.  Attempting to
        // persist it a second time here would cause the state-service commit
        // handler to fire twice, producing duplicate MPT trie entries and an
        // incorrect state root.  We therefore only log the current state.
        tracing::debug!(
            target: "neo",
            persisted_index,
            persisted_hash = %persisted_hash,
            "blockchain actor initialized; genesis already persisted by NeoSystem"
        );
    }
}
