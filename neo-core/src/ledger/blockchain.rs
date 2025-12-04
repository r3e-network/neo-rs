//! Blockchain actor for block processing and chain management.
//!
//! This module implements the blockchain actor that handles block validation,
//! persistence, and chain synchronization, mirroring the C# `Neo.Ledger.Blockchain`.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Blockchain Actor                          │
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                   Message Handlers                       ││
//! │  │  ┌──────────┐  ┌──────────┐  ┌──────────────────────┐  ││
//! │  │  │ Import   │  │ FillMem  │  │ Reverify             │  ││
//! │  │  │ (blocks) │  │ Pool     │  │ (transactions)       │  ││
//! │  │  └──────────┘  └──────────┘  └──────────────────────┘  ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │  ┌─────────────────────────────────────────────────────────┐│
//! │  │                   Block Processing                       ││
//! │  │  ┌──────────┐  ┌──────────┐  ┌──────────────────────┐  ││
//! │  │  │ Verify   │  │ Persist  │  │ Relay                │  ││
//! │  │  │ Block    │  │ Block    │  │ (to peers)           │  ││
//! │  │  └──────────┘  └──────────┘  └──────────────────────┘  ││
//! │  └─────────────────────────────────────────────────────────┘│
//! │  ┌──────────────┐  ┌──────────────┐  ┌──────────────────┐  │
//! │  │ Block Cache  │  │ Unverified   │  │ Extensible       │  │
//! │  │ (verified)   │  │ Blocks       │  │ Whitelist        │  │
//! │  └──────────────┘  └──────────────┘  └──────────────────┘  │
//! └─────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Key Components
//!
//! - [`Blockchain`]: Actor managing block import and chain state
//! - [`BlockchainCommand`]: Messages for block import, mempool fill, reverification
//! - [`LedgerContext`]: Shared ledger state (headers, blocks, transactions)
//!
//! # Block Processing Flow
//!
//! 1. Receive block via `Import` command
//! 2. Validate block header and transactions
//! 3. Execute transactions via ApplicationEngine
//! 4. Persist block and update chain state
//! 5. Relay block to connected peers
//! 6. Emit plugin events (OnPersist, OnCommit)
//!
//! # Caching Strategy
//!
//! - **Block Cache**: Verified blocks awaiting persistence
//! - **Unverified Cache**: Blocks received out of order, pending verification
//! - **Extensible Whitelist**: Authorized senders for extensible payloads

use crate::error::CoreError;
use crate::extensions::plugin::PluginEvent;
use crate::ledger::LedgerContext;
use crate::neo_io::{MemoryReader, Serializable};
use crate::neo_system::NeoSystemContext;
use crate::network::p2p::{
    local_node::RelayInventory,
    payloads::{
        block::Block, extensible_payload::ExtensiblePayload, header::Header,
        inventory_type::InventoryType, Transaction,
    },
    LocalNodeCommand,
};
use crate::persistence::StoreCache;
use crate::smart_contract::native::LedgerContract;
use crate::state_service::{StateRoot, STATE_SERVICE_CATEGORY};
use crate::{UInt160, UInt256};
use akka::{Actor, ActorContext, ActorResult, Props};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use super::VerifyResult;

const MAX_TX_TO_REVERIFY_PER_IDLE: usize = 10;

/// Rust analogue of `Neo.Ledger.Blockchain` (actor based on Akka).
pub struct Blockchain {
    ledger: Arc<LedgerContext>,
    system_context: Option<Arc<NeoSystemContext>>,
    _block_cache: Arc<RwLock<HashMap<UInt256, Block>>>,
    _block_cache_unverified: Arc<RwLock<HashMap<u32, UnverifiedBlocksList>>>,
    _extensible_witness_white_list: Arc<RwLock<HashSet<UInt160>>>,
}

impl Blockchain {
    pub fn new(ledger: Arc<LedgerContext>) -> Self {
        Self {
            ledger,
            system_context: None,
            _block_cache: Arc::new(RwLock::new(HashMap::new())),
            _block_cache_unverified: Arc::new(RwLock::new(HashMap::new())),
            _extensible_witness_white_list: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    pub fn props(ledger: Arc<LedgerContext>) -> Props {
        Props::new(move || Self::new(ledger.clone()))
    }

    fn persist_block_via_system(&self, block: &Block) {
        let Some(context) = &self.system_context else {
            return;
        };

        let Some(system) = context.neo_system() else {
            return;
        };

        let mut block_for_hash = block.clone();
        let hash = block_for_hash.hash();

        if let Err(error) = system.persist_block(block.clone()) {
            tracing::warn!(
                target: "neo",
                %error,
                index = block.index(),
                hash = %hash,
                "failed to persist block via NeoSystem"
            );
        }
    }

    fn deserialize_inventory<T>(payload: &[u8]) -> Option<T>
    where
        T: Serializable,
    {
        let mut reader = MemoryReader::new(payload);
        T::deserialize(&mut reader).ok()
    }

    async fn handle_persist_completed(&self, persist: PersistCompleted) {
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
            if let Ok(mut pool) = context.memory_pool().lock() {
                pool.update_pool_for_block_persisted(&block);
            }
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

    fn handle_headers(&self, headers: Vec<Header>) {
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

    async fn handle_import(&self, import: Import, ctx: &ActorContext) {
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

    async fn handle_fill_memory_pool(&self, fill: FillMemoryPool, ctx: &ActorContext) {
        if let Some(context) = &self.system_context {
            let store_cache = context.store_cache();
            let settings = context.settings();
            if let Ok(mut pool) = context.memory_pool().lock() {
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
            }

            if let Some(sender) = ctx.sender() {
                let _ = sender.tell(FillCompleted);
            }
        }
    }

    async fn handle_reverify(&self, reverify: Reverify, ctx: &ActorContext) {
        for item in &reverify.inventories {
            match item.inventory_type {
                InventoryType::Block => {
                    if let Some(block) = Self::deserialize_inventory::<Block>(&item.payload) {
                        if let Err(error) = self.handle_block_inventory(block, false, ctx).await {
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
                    if let Some(tx) = Self::deserialize_inventory::<Transaction>(&item.payload) {
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
                        Self::deserialize_inventory::<ExtensiblePayload>(&item.payload)
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
            }
        }

        if let Some(context) = &self.system_context {
            let store_cache = context.store_cache();
            let settings = context.settings();
            let header_cache = context.header_cache();
            let header_backlog = header_cache.count() > 0 || self.ledger.has_future_headers();
            if let Ok(mut pool) = context.memory_pool().lock() {
                let snapshot = store_cache.data_cache();
                let max_to_verify = reverify.inventories.len().max(1);
                let more_pending = pool.reverify_top_unverified_transactions(
                    max_to_verify,
                    snapshot,
                    &settings,
                    header_backlog,
                );
                drop(pool);

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
    }

    async fn handle_block_inventory(
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

    async fn handle_extensible_inventory(
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

    async fn on_new_block(&self, block: &Block, verify: bool) -> VerifyResult {
        let Some(context) = &self.system_context else {
            return VerifyResult::Invalid;
        };

        let block_index = block.index();
        let hash = {
            let mut temp = block.clone();
            temp.hash()
        };

        let store_cache = context.store_cache();
        let settings = context.settings();
        let header_cache = context.header_cache();

        let current_height = context.ledger().current_height();
        let header_height = header_cache
            .last()
            .map(|header| header.index())
            .unwrap_or(current_height);

        if block_index <= current_height {
            return VerifyResult::AlreadyExists;
        }

        if block_index > header_height + 1 {
            self.add_unverified_block(block.clone()).await;
            return VerifyResult::UnableToVerify;
        }

        if verify {
            if block_index == header_height + 1 {
                if !block.verify_with_cache(settings.as_ref(), &store_cache, &header_cache) {
                    tracing::warn!(
                        target: "neo",
                        index = block_index,
                        %hash,
                        prev = %block.prev_hash(),
                        "block verification failed against header cache"
                    );
                    return VerifyResult::Invalid;
                }
            } else {
                let Some(mut header) = header_cache.get(block_index) else {
                    tracing::warn!(
                        target: "neo",
                        index = block_index,
                        "header entry missing for block"
                    );
                    return VerifyResult::Invalid;
                };

                if header.hash() != hash {
                    tracing::warn!(
                        target: "neo",
                        index = block_index,
                        %hash,
                        "block hash does not match cached header"
                    );
                    return VerifyResult::Invalid;
                }
            }
        }

        // Use write lock directly to prevent race condition where another
        // thread could insert the same block between read check and write insert.
        {
            let mut cache = self._block_cache.write().await;
            if cache.contains_key(&hash) {
                return VerifyResult::AlreadyExists;
            }
            cache.insert(hash, block.clone());
        }

        if block_index == current_height + 1 {
            self.persist_block_sequence(block.clone()).await;
            VerifyResult::Succeed
        } else {
            if block_index == header_height + 1 {
                header_cache.add(block.header.clone());
            }
            self.add_unverified_block(block.clone()).await;
            VerifyResult::Succeed
        }
    }

    async fn add_unverified_block(&self, block: Block) {
        let mut unverified = self._block_cache_unverified.write().await;
        let entry = unverified
            .entry(block.index())
            .or_insert_with(UnverifiedBlocksList::new);
        entry.blocks.push(block);
    }

    async fn persist_block_sequence(&self, block: Block) {
        self.persist_block_via_system(&block);
        self.handle_persist_completed(PersistCompleted {
            block: block.clone(),
        })
        .await;

        let mut next_index = block.index().saturating_add(1);

        loop {
            let maybe_block = {
                let mut unverified = self._block_cache_unverified.write().await;
                if let Some(entry) = unverified.get_mut(&next_index) {
                    if let Some(next_block) = entry.blocks.pop() {
                        if entry.blocks.is_empty() {
                            unverified.remove(&next_index);
                        }
                        Some(next_block)
                    } else {
                        unverified.remove(&next_index);
                        None
                    }
                } else {
                    None
                }
            };

            let Some(next_block) = maybe_block else {
                break;
            };

            self.persist_block_via_system(&next_block);
            self.handle_persist_completed(PersistCompleted {
                block: next_block.clone(),
            })
            .await;
            next_index = next_index.saturating_add(1);
        }
    }

    async fn on_new_extensible(&self, payload: ExtensiblePayload) -> VerifyResult {
        let Some(context) = &self.system_context else {
            return VerifyResult::Invalid;
        };

        let store_cache = context.store_cache();
        let settings = context.settings();

        {
            let mut whitelist = self._extensible_witness_white_list.write().await;
            whitelist.insert(payload.sender);
        }

        let whitelist = self._extensible_witness_white_list.read().await;
        let snapshot = store_cache.data_cache();

        if !payload.verify(settings.as_ref(), snapshot, &whitelist) {
            return VerifyResult::Invalid;
        }

        if payload.category == STATE_SERVICE_CATEGORY {
            match self.process_state_service_payload(context, &payload) {
                Ok(true) => {}
                Ok(false) => return VerifyResult::Invalid,
                Err(err) => {
                    warn!(target: "neo", %err, "state service payload handling failed");
                    return VerifyResult::Invalid;
                }
            }
        }

        context.record_extensible(payload);
        VerifyResult::Succeed
    }

    fn transaction_exists_on_chain(&self, tx: &Transaction, snapshot: &StoreCache) -> bool {
        LedgerContract::new()
            .contains_transaction(snapshot, &tx.hash())
            .unwrap_or(false)
    }

    fn conflict_exists_on_chain(
        &self,
        tx: &Transaction,
        snapshot: &StoreCache,
        max_traceable_blocks: u32,
    ) -> bool {
        let signers: Vec<UInt160> = tx.signers().iter().map(|signer| signer.account).collect();
        if signers.is_empty() {
            return false;
        }

        LedgerContract::new()
            .contains_conflict_hash(snapshot, &tx.hash(), &signers, max_traceable_blocks)
            .unwrap_or(false)
    }

    fn process_state_service_payload(
        &self,
        context: &Arc<NeoSystemContext>,
        payload: &ExtensiblePayload,
    ) -> Result<bool, CoreError> {
        if payload.data.is_empty() {
            return Ok(false);
        }

        // MessageType::StateRoot = 0
        if payload.data[0] != 0 {
            return Ok(false);
        }

        let mut reader = MemoryReader::new(&payload.data[1..]);
        let state_root = <StateRoot as Serializable>::deserialize(&mut reader)
            .map_err(|err| CoreError::invalid_data(err.to_string()))?;

        let Some(state_store) = context.state_store()? else {
            return Err(CoreError::system("state store service not registered"));
        };

        let accepted = state_store.on_new_state_root(state_root.clone());
        if !accepted {
            debug!(
                target: "state",
                index = state_root.index,
                "state service payload rejected by StateStore"
            );
        }
        Ok(accepted)
    }

    async fn handle_idle(&self, ctx: &ActorContext) {
        if let Some(system_context) = &self.system_context {
            let store_cache = system_context.store_cache();
            let settings = system_context.settings();
            if let Ok(mut pool) = system_context.memory_pool().lock() {
                let snapshot = store_cache.data_cache();
                let header_backlog = self.ledger.has_future_headers();
                let more_pending = pool.reverify_top_unverified_transactions(
                    MAX_TX_TO_REVERIFY_PER_IDLE,
                    snapshot,
                    &settings,
                    header_backlog,
                );
                drop(pool);

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
    }

    fn on_new_transaction(&self, transaction: &Transaction) -> VerifyResult {
        let Some(context) = &self.system_context else {
            return VerifyResult::Invalid;
        };

        let hash = transaction.hash();

        let memory_pool = context.memory_pool_handle();
        if let Ok(pool) = memory_pool.lock() {
            if pool.contains_key(&hash) {
                return VerifyResult::AlreadyInPool;
            }
        }

        let store_cache = context.store_cache();
        let ledger_contract = LedgerContract::new();
        if ledger_contract
            .contains_transaction(&store_cache, &hash)
            .unwrap_or(false)
        {
            return VerifyResult::AlreadyExists;
        }

        let signers: Vec<UInt160> = transaction
            .signers()
            .iter()
            .map(|signer| signer.account)
            .collect();
        if !signers.is_empty() {
            let settings = context.protocol_settings();
            let max_traceable = ledger_contract
                .max_traceable_blocks_snapshot(&store_cache, &settings)
                .unwrap_or(settings.max_traceable_blocks);

            if ledger_contract
                .contains_conflict_hash(&store_cache, &hash, &signers, max_traceable)
                .unwrap_or(false)
            {
                return VerifyResult::HasConflicts;
            }
        }

        let snapshot = store_cache.data_cache();
        let settings = context.protocol_settings();

        let add_result = match memory_pool.lock() {
            Ok(mut pool) => pool.try_add(transaction.clone(), snapshot, &settings),
            Err(_) => VerifyResult::Invalid,
        };

        add_result
    }

    async fn handle_preverify_completed(&self, task: PreverifyCompleted, ctx: &ActorContext) {
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

    async fn handle_relay_result(&self, _result: RelayResult) {}

    async fn initialize(&self) {
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

#[allow(dead_code)]
#[derive(Debug, Clone)]
struct UnverifiedBlocksList {
    blocks: Vec<Block>,
    nodes: HashSet<String>,
}

impl UnverifiedBlocksList {
    #[allow(dead_code)]
    fn new() -> Self {
        Self {
            blocks: Vec::new(),
            nodes: HashSet::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersistCompleted {
    pub block: Block,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub blocks: Vec<Block>,
    pub verify: bool,
}

impl Default for Import {
    fn default() -> Self {
        Self {
            blocks: Vec::new(),
            verify: true,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportCompleted;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillMemoryPool {
    pub transactions: Vec<Transaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillCompleted;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReverifyItem {
    pub inventory_type: InventoryType,
    pub payload: Vec<u8>,
    #[serde(default)]
    pub block_index: Option<u32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Reverify {
    pub inventories: Vec<ReverifyItem>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ImportDisposition {
    AlreadySeen,
    NextExpected,
    FutureGap,
}

fn classify_import_block(current_height: u32, block_index: u32) -> ImportDisposition {
    if block_index <= current_height {
        ImportDisposition::AlreadySeen
    } else if block_index == current_height.saturating_add(1) {
        ImportDisposition::NextExpected
    } else {
        ImportDisposition::FutureGap
    }
}

fn should_schedule_reverify_idle(more_pending: bool, header_backlog: bool) -> bool {
    more_pending && !header_backlog
}

pub use crate::ledger::transaction_router::PreverifyCompleted;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayResult {
    pub hash: UInt256,
    pub inventory_type: InventoryType,
    pub block_index: Option<u32>,
    pub result: VerifyResult,
}

#[derive(Debug, Clone)]
pub enum BlockchainCommand {
    PersistCompleted(PersistCompleted),
    Import(Import),
    FillMemoryPool(FillMemoryPool),
    FillCompleted,
    Reverify(Reverify),
    InventoryBlock {
        block: Block,
        relay: bool,
    },
    InventoryExtensible {
        payload: ExtensiblePayload,
        relay: bool,
    },
    PreverifyCompleted(PreverifyCompleted),
    Headers(Vec<Header>),
    Idle,
    RelayResult(RelayResult),
    Initialize,
    AttachSystem(Arc<NeoSystemContext>),
}

#[cfg(test)]
mod tests {
    use super::{
        classify_import_block, should_schedule_reverify_idle, Blockchain, ImportDisposition,
        StateRoot, STATE_SERVICE_CATEGORY,
    };
    use crate::neo_io::BinaryWriter;
    use crate::network::p2p::payloads::extensible_payload::ExtensiblePayload;
    use crate::network::p2p::payloads::witness::Witness as PayloadWitness;
    use crate::smart_contract::Contract;
    use crate::wallets::KeyPair;
    use crate::{neo_io::Serializable, NeoSystem, ProtocolSettings};
    use neo_vm::op_code::OpCode;

    #[test]
    fn classify_import_block_returns_already_seen_for_past_height() {
        assert_eq!(classify_import_block(10, 5), ImportDisposition::AlreadySeen);
        assert_eq!(
            classify_import_block(10, 10),
            ImportDisposition::AlreadySeen
        );
    }

    #[test]
    fn classify_import_block_returns_next_expected_when_in_sequence() {
        assert_eq!(classify_import_block(7, 8), ImportDisposition::NextExpected);
    }

    #[test]
    fn classify_import_block_detects_future_gap() {
        assert_eq!(classify_import_block(3, 8), ImportDisposition::FutureGap);
    }

    #[test]
    fn schedule_idle_only_when_more_pending_without_backlog() {
        assert!(should_schedule_reverify_idle(true, false));
        assert!(!should_schedule_reverify_idle(false, false));
        assert!(!should_schedule_reverify_idle(true, true));
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn state_service_payload_ingests_into_shared_state_store() {
        let mut settings = ProtocolSettings::mainnet();
        let keypair = KeyPair::generate().expect("generate keypair");
        let validator = keypair
            .get_public_key_point()
            .expect("public key point from keypair");
        settings.standby_committee = vec![validator.clone()];
        settings.validators_count = 1;
        settings.network = 0x42_4242;

        let system =
            NeoSystem::new(settings.clone(), None, None).expect("NeoSystem::new should succeed");
        let state_store = system
            .state_store()
            .expect("state store lookup")
            .expect("state store registered");

        let height = 5;
        state_store.update_local_state_root_snapshot(height, std::iter::empty());
        state_store.update_local_state_root(height);
        let root_hash = state_store
            .current_local_root_hash()
            .expect("local root hash seeded");

        let mut state_root = StateRoot::new_current(height, root_hash);
        let hash = state_root.hash();
        let mut sign_data = Vec::with_capacity(4 + hash.to_bytes().len());
        sign_data.extend_from_slice(&settings.network.to_le_bytes());
        sign_data.extend_from_slice(&hash.to_array());
        let signature = keypair.sign(&sign_data).expect("sign state root");
        let mut invocation = Vec::with_capacity(signature.len() + 2);
        invocation.push(OpCode::PUSHDATA1 as u8);
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(&signature);
        let verification_script = Contract::create_multi_sig_redeem_script(1, &[validator]);
        state_root.witness = Some(PayloadWitness::new_with_scripts(
            invocation,
            verification_script,
        ));

        let mut writer = BinaryWriter::new();
        state_root
            .serialize(&mut writer)
            .expect("serialize state root");
        let mut payload_bytes = vec![0u8];
        payload_bytes.extend_from_slice(&writer.into_bytes());

        let mut payload = ExtensiblePayload::new();
        payload.category = STATE_SERVICE_CATEGORY.to_string();
        payload.valid_block_start = 0;
        payload.valid_block_end = height + 10;
        payload.sender = keypair.get_script_hash();
        payload.data = payload_bytes;

        let blockchain = Blockchain::new(system.ledger_context());
        let accepted = blockchain
            .process_state_service_payload(&system.context(), &payload)
            .expect("state service payload");
        assert!(accepted);
        assert_eq!(state_store.validated_root_index(), Some(height));
    }
}
