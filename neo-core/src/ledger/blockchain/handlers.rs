//
// handlers.rs - Message handlers for Blockchain actor
//

use super::*;

impl Blockchain {
    pub(super) async fn handle_persist_completed(&self, persist: PersistCompleted) {
        let PersistCompleted { mut block } = persist;
        let hash = block.hash();
        let index = block.index();
        let tx_count = block.transactions.len();
        tracing::debug!(
            target: "neo",
            %hash,
            index,
            tx_count,
            "persist completed for block"
        );

        {
            let mut cache = self._block_cache.write().await;
            cache.insert(hash, block.clone());

            let prev_hash = *block.prev_hash();
            if !prev_hash.is_zero() {
                cache.remove(&prev_hash);
            }
        }

        self.ledger.insert_block(block.clone());

        for transaction in &block.transactions {
            let tx_hash = transaction.hash();
            self.ledger.remove_transaction(&tx_hash);
        }

        if let Some(context) = &self.system_context {
            context
                .memory_pool()
                .lock()
                .update_pool_for_block_persisted(&block);
        }

        if let Some(context) = &self.system_context {
            context
                .actor_system
                .event_stream()
                .publish(PersistCompleted {
                    block: block.clone(),
                });
        }

        {
            let mut unverified = self._block_cache_unverified.write().await;
            unverified.remove(&index);
        }

        if let Some(context) = &self.system_context {
            context.header_cache().remove_up_to(index);
        }

        self._extensible_witness_white_list.write().await.clear();
    }

    pub(super) fn handle_headers(&self, headers: Vec<Header>) {
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

        for header in headers.into_iter() {
            let index = header.index();
            if index <= header_height {
                continue;
            }

            if index != header_height + 1 {
                break;
            }

            if !header.verify_with_cache(settings.as_ref(), &store_cache, &header_cache) {
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
                let _ = sender.tell(ImportCompleted);
            }
            return;
        };

        let settings = context.settings();
        let store_cache = context.store_cache();
        let ledger_contract = LedgerContract::new();
        let mut current_height = ledger_contract
            .current_index(&store_cache)
            .unwrap_or_else(|_| context.ledger().current_height());

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

            if import.verify && !block.verify(settings.as_ref(), &store_cache) {
                tracing::warn!(
                    target: "neo",
                    height = index,
                    "import block failed verification"
                );
                break;
            }

            self.persist_block_via_system(&block);
            self.handle_persist_completed(PersistCompleted {
                block: block.clone(),
            })
            .await;
            current_height = index;
        }

        if let Some(sender) = ctx.sender() {
            let _ = sender.tell(ImportCompleted);
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

                let tx_hash = tx.hash();
                let _ = pool.remove_unverified(&tx_hash);

                let _ = pool.try_add(tx, snapshot, &settings);
            }

            let needs_idle = pool.unverified_count() > 0;
            drop(pool);

            if needs_idle {
                if let Err(error) = ctx.self_ref().tell(BlockchainCommand::Idle) {
                    tracing::debug!(
                        target: "neo",
                        %error,
                        "failed to enqueue idle reverify after filling memory pool"
                    );
                }
            }

            if let Some(sender) = ctx.sender() {
                let _ = sender.tell(FillCompleted);
            }
        }
    }

    pub(super) async fn handle_reverify(&self, reverify: Reverify, ctx: &ActorContext) {
        let max_to_verify = reverify.inventories.len().max(1);

        for item in reverify.inventories {
            match item.payload {
                InventoryPayload::Block(block) => {
                    if let Err(error) = self.handle_block_inventory(*block, false, ctx).await {
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
                    if let Err(error) = self
                        .handle_extensible_inventory(*payload, false, ctx)
                        .await
                    {
                        tracing::debug!(
                            target: "neo",
                            %error,
                            "failed to reverify extensible payload"
                        );
                    }
                }
                InventoryPayload::Raw(inventory_type, payload) => match inventory_type {
                    InventoryType::Block => {
                        if let Some(block) = Self::deserialize_inventory::<Block>(&payload) {
                            if let Err(error) = self.handle_block_inventory(block, false, ctx).await
                            {
                                tracing::debug!(
                                    target: "neo",
                                    %error,
                                    "failed to reverify block inventory"
                                );
                            }
                        } else {
                            tracing::debug!(
                                target: "neo",
                                "failed to deserialize block payload during reverify"
                            );
                        }
                    }
                    InventoryType::Transaction => {
                        if let Some(tx) = Self::deserialize_inventory::<Transaction>(&payload) {
                            let _ = self.on_new_transaction(&tx);
                        } else {
                            tracing::debug!(
                                target: "neo",
                                "failed to deserialize transaction payload during reverify"
                            );
                        }
                    }
                    InventoryType::Consensus | InventoryType::Extensible => {
                        if let Some(payload) =
                            Self::deserialize_inventory::<ExtensiblePayload>(&payload)
                        {
                            if let Err(error) =
                                self.handle_extensible_inventory(payload, false, ctx).await
                            {
                                tracing::debug!(
                                    target: "neo",
                                    %error,
                                    "failed to reverify extensible payload"
                                );
                            }
                        } else {
                            tracing::debug!(
                                target: "neo",
                                "failed to deserialize extensible payload during reverify"
                            );
                        }
                    }
                },
            }
        }

        if let Some(context) = &self.system_context {
            let store_cache = context.store_cache();
            let settings = context.settings();
            let header_cache = context.header_cache();
            let header_backlog = header_cache.count() > 0 || self.ledger.has_future_headers();
            let snapshot = store_cache.data_cache();
            let more_pending = context
                .memory_pool()
                .lock()
                .reverify_top_unverified_transactions(
                    max_to_verify,
                    snapshot,
                    &settings,
                    header_backlog,
                );

            if should_schedule_reverify_idle(more_pending, header_backlog) {
                if let Err(error) = ctx.self_ref().tell(BlockchainCommand::Idle) {
                    tracing::debug!(
                        target: "neo",
                        %error,
                        "failed to enqueue idle reverify after reverify command"
                    );
                }
            }
        }
    }

    pub(super) async fn handle_block_inventory(
        &self,
        mut block: Block,
        relay: bool,
        ctx: &ActorContext,
    ) -> ActorResult {
        let hash = block.hash();
        let index = block.index();

        let result = self.on_new_block(&block, true).await;

        if let Some(context) = &self.system_context {
            let inventory = if relay && result == VerifyResult::Succeed {
                Some(RelayInventory::Block(block.clone()))
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
        payload: ExtensiblePayload,
        relay: bool,
        ctx: &ActorContext,
    ) -> ActorResult {
        let mut payload_for_hash = payload.clone();
        let hash = payload_for_hash.hash();

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
            let header_backlog = self.ledger.has_future_headers();
            let more_pending = system_context
                .memory_pool()
                .lock()
                .reverify_top_unverified_transactions(
                    MAX_TX_TO_REVERIFY_PER_IDLE,
                    snapshot,
                    &settings,
                    header_backlog,
                );

            if more_pending {
                if let Err(error) = ctx.self_ref().tell(BlockchainCommand::Idle) {
                    tracing::debug!(
                        target: "neo",
                        %error,
                        "failed to enqueue idle reverify continuation"
                    );
                }
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

        let tx_hash = task.transaction.hash();

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
        if relay && result == VerifyResult::Succeed {
            if let Some(inv) = inventory {
                if let Err(error) = context.local_node.tell(LocalNodeCommand::RelayDirectly {
                    inventory: inv,
                    block_index,
                }) {
                    tracing::debug!(
                        target: "neo",
                        %error,
                        "failed to record relay broadcast"
                    );
                }
            }
        }

        let relay_message = RelayResult {
            hash,
            inventory_type,
            block_index,
            result,
        };

        context
            .actor_system
            .event_stream()
            .publish(relay_message.clone());

        if result == VerifyResult::Succeed && matches!(inventory_type, InventoryType::Transaction) {
            context.broadcast_plugin_event(PluginEvent::TransactionReceived {
                tx_hash: hash.to_string(),
            });
        }

        if let Some(sender) = ctx.sender() {
            if let Err(error) = sender.tell(relay_message) {
                tracing::debug!(
                    target: "neo",
                    %error,
                    "failed to reply with relay result to sender"
                );
            }
        }
    }

    pub(super) async fn handle_relay_result(&self, _result: RelayResult) {}

    pub(super) async fn initialize(&self) {
        let Some(context) = &self.system_context else {
            tracing::debug!(target: "neo", "blockchain initialize requested before context attached");
            return;
        };

        let ledger = context.ledger();
        if ledger.block_hash_at(0).is_some() {
            tracing::debug!(target: "neo", "ledger already contains genesis block; skipping initialization");
            return;
        }

        let genesis = context.genesis_block();
        let block = genesis.as_ref().clone();
        tracing::info!(target: "neo", "persisting genesis block during initialization");
        self.persist_block_via_system(&block);
        self.handle_persist_completed(PersistCompleted { block })
            .await;
    }
}
