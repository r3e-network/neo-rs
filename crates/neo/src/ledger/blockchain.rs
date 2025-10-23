use crate::ledger::LedgerContext;
use crate::neo_system::NeoSystemContext;
use crate::network::p2p::payloads::{block::Block, Transaction};
use crate::persistence::StoreCache;
use crate::smart_contract::native::LedgerContract;
use crate::{UInt160, UInt256};
use akka::{Actor, ActorContext, ActorResult, Props};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

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

        self._extensible_witness_white_list.write().await.clear();
    }

    async fn handle_import(&self, import: Import) {
        for mut block in import.blocks {
            let hash = block.hash();

            if import.verify {
                tracing::debug!(target: "neo", %hash, "verifying block prior to import");
                // Full verification logic will be ported alongside the consensus pipeline.
            }

            let mut cache = self._block_cache.write().await;
            cache.insert(hash, block.clone());

            self.handle_persist_completed(PersistCompleted { block })
                .await;
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
        if let Some(context) = &self.system_context {
            let store_cache = context.store_cache();
            let settings = context.settings();
            if let Ok(mut pool) = context.memory_pool().lock() {
                let snapshot = store_cache.data_cache();
                let max_to_verify = reverify.inventories.len().max(1);
                let header_backlog = self.ledger.has_future_headers();
                let more_pending = pool.reverify_top_unverified_transactions(
                    max_to_verify,
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
                            "failed to enqueue idle reverify after reverify command"
                        );
                    }
                }
            }
        }
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
                BlockchainCommand::Import(import) => self.handle_import(import).await,
                BlockchainCommand::FillMemoryPool(fill) => {
                    self.handle_fill_memory_pool(fill, ctx).await
                }
                BlockchainCommand::Reverify(reverify) => self.handle_reverify(reverify, ctx).await,
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

#[derive(Debug, Clone)]
struct UnverifiedBlocksList {
    blocks: Vec<Block>,
    nodes: HashSet<String>,
}

impl UnverifiedBlocksList {
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
pub struct Reverify {
    pub inventories: Vec<Transaction>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayResult {
    pub inventory: Transaction,
    pub result: VerifyResult,
}

#[derive(Debug, Clone)]
pub enum BlockchainCommand {
    PersistCompleted(PersistCompleted),
    Import(Import),
    FillMemoryPool(FillMemoryPool),
    FillCompleted,
    Reverify(Reverify),
    Idle,
    RelayResult(RelayResult),
    Initialize,
    AttachSystem(Arc<NeoSystemContext>),
}
