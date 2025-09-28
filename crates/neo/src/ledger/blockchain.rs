use crate::network::p2p::payloads::Transaction;
use crate::{UInt160, UInt256};
use akka::{Actor, ActorContext, ActorResult, Props};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::RwLock;

use super::VerifyResult;

/// Rust analogue of `Neo.Ledger.Blockchain` (actor based on Akka).
pub struct Blockchain {
    system: Arc<()>,
    _block_cache: Arc<RwLock<HashMap<UInt256, Transaction>>>,
    _block_cache_unverified: Arc<RwLock<HashMap<u32, UnverifiedBlocksList>>>,
    _extensible_witness_white_list: Arc<RwLock<HashSet<UInt160>>>,
}

impl Blockchain {
    pub fn new(system: Arc<()>) -> Self {
        Self {
            system,
            _block_cache: Arc::new(RwLock::new(HashMap::new())),
            _block_cache_unverified: Arc::new(RwLock::new(HashMap::new())),
            _extensible_witness_white_list: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    pub fn props(system: Arc<()>) -> Props {
        Props::new(move || Self::new(system.clone()))
    }

    async fn handle_persist_completed(&self, _persist: PersistCompleted) {}

    async fn handle_import(&self, _import: Import) {}

    async fn handle_fill_memory_pool(&self, _fill: FillMemoryPool) {}

    async fn handle_relay_result(&self, _result: RelayResult) {}

    async fn initialize(&self) {}
}

#[async_trait]
impl Actor for Blockchain {
    async fn pre_start(&mut self, _ctx: &mut ActorContext) -> ActorResult {
        self.initialize().await;
        Ok(())
    }

    async fn handle(
        &mut self,
        envelope: Box<dyn Any + Send>,
        _ctx: &mut ActorContext,
    ) -> ActorResult {
        if let Ok(message) = envelope.downcast::<BlockchainCommand>() {
            match *message {
                BlockchainCommand::PersistCompleted(persist) => {
                    self.handle_persist_completed(persist).await
                }
                BlockchainCommand::Import(import) => self.handle_import(import).await,
                BlockchainCommand::FillMemoryPool(fill) => self.handle_fill_memory_pool(fill).await,
                BlockchainCommand::RelayResult(result) => self.handle_relay_result(result).await,
                BlockchainCommand::Initialize => self.initialize().await,
            }
            Ok(())
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone)]
struct UnverifiedBlocksList {
    blocks: Vec<Transaction>,
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
    pub block: Transaction,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Import {
    pub blocks: Vec<Transaction>,
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
    RelayResult(RelayResult),
    Initialize,
}
