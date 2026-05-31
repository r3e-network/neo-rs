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
use crate::events::PluginEvent;
use crate::ledger::LedgerContext;
use crate::neo_io::{MemoryReader, Serializable};
use crate::services::SystemContext;
use crate::network::p2p::{
    local_node::RelayInventory,
    payloads::{
        InventoryType, Transaction, block::Block, extensible_payload::ExtensiblePayload,
        header::Header,
    },
};
use crate::persistence::DataCache;
use crate::persistence::StoreCache;
use crate::protocol_settings::ProtocolSettings;
use crate::runtime::{Actor, ActorContext, ActorResult, Props, ScheduleHandle};
use crate::smart_contract::native::LedgerContract;
use crate::state_service::{STATE_SERVICE_CATEGORY, StateRoot};
use crate::{CoreResult, UInt160, UInt256};
use async_trait::async_trait;
use dashmap::DashMap;
use lru::LruCache;
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use super::VerifyResult;
use internal::{ImportDisposition, UnverifiedBlocksList, classify_import_block};

#[cfg(test)]
use internal::should_schedule_reverify_idle;

const MAX_TX_TO_REVERIFY_PER_IDLE: usize = 10;
const MAX_REVERIFY_INVENTORY_CACHE: usize = 256;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
struct InventoryCacheKey {
    inventory_type: InventoryType,
    payload_hash: UInt256,
}

impl InventoryCacheKey {
    fn new(inventory_type: InventoryType, payload: &[u8]) -> Self {
        let payload_hash = UInt256::from(neo_crypto::Crypto::sha256(payload));
        Self {
            inventory_type,
            payload_hash,
        }
    }
}

/// Maximum number of verified blocks to keep in the block cache.
/// Prevents unbounded memory growth from out-of-order or attacker-injected blocks.
/// Sized to accommodate the fast sync download window without backpressure.
const MAX_BLOCK_CACHE_SIZE: usize = 20000;

/// Maximum number of index entries in the unverified block cache.
/// Sized at 2x the sync window to prevent eviction of blocks near the
/// persistence front when multiple sessions deliver overlapping ranges.
const MAX_UNVERIFIED_CACHE_SIZE: usize = 20000;

/// Rust analogue of `Neo.Ledger.Blockchain` using the async actor runtime.
pub struct Blockchain {
    ledger: Arc<LedgerContext>,
    system_context: Option<Arc<dyn SystemContext>>,
    _block_cache: Arc<DashMap<UInt256, Arc<Block>>>,
    _block_cache_unverified: Arc<DashMap<u32, UnverifiedBlocksList>>,
    _extensible_witness_white_list: Arc<RwLock<HashSet<UInt160>>>,
    _inventory_cache: Arc<RwLock<LruCache<InventoryCacheKey, InventoryPayload>>>,
    _drain_timer: Option<ScheduleHandle>,
}

impl Blockchain {
    pub fn new(ledger: Arc<LedgerContext>) -> Self {
        Self {
            ledger,
            system_context: None,
            _block_cache: Arc::new(DashMap::with_capacity(1024)),
            _block_cache_unverified: Arc::new(DashMap::with_capacity(256)),
            _extensible_witness_white_list: Arc::new(RwLock::new(HashSet::new())),
            _inventory_cache: Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(MAX_REVERIFY_INVENTORY_CACHE)
                    .expect("inventory cache capacity is non-zero"),
            ))),
            _drain_timer: None,
        }
    }

    pub fn props(ledger: Arc<LedgerContext>) -> Props {
        Props::new(move || Self::new(ledger.clone()))
    }

    fn try_block_hash(block: &Block) -> CoreResult<UInt256> {
        let mut header = block.header.clone();
        header.try_hash()
    }

    fn persist_block_via_system(&self, block: &Arc<Block>) -> bool {
        let Some(context) = &self.system_context else {
            return false;
        };

        let Some(system) = context.neo_system() else {
            return false;
        };

        let hash = match Self::try_block_hash(block.as_ref()) {
            Ok(hash) => hash,
            Err(error) => {
                tracing::warn!(
                    target: "neo",
                    error = %error,
                    index = block.index(),
                    "failed to compute block hash before persistence"
                );
                return false;
            }
        };

        match system.persist_block((**block).clone()) {
            Ok(_) => {
                tracing::debug!(
                    target: "neo",
                    index = block.index(),
                    hash = %hash,
                    "persisted block successfully"
                );
                true
            }
            Err(error) => {
                tracing::warn!(
                    target: "neo",
                    %error,
                    index = block.index(),
                    hash = %hash,
                    "failed to persist block via NeoSystem"
                );
                // In fast sync mode, we continue even if blocks fail
                // The block might fail due to gas/balance issues but we can still sync
                false
            }
        }
    }

    fn deserialize_inventory<T>(payload: &[u8]) -> Option<T>
    where
        T: Serializable,
    {
        let mut reader = MemoryReader::new(payload);
        T::deserialize(&mut reader).ok()
    }

    async fn inventory_cache_get(&self, key: &InventoryCacheKey) -> Option<InventoryPayload> {
        self._inventory_cache.read().await.peek(key).cloned()
    }

    async fn inventory_cache_insert(&self, key: InventoryCacheKey, payload: InventoryPayload) {
        let mut cache = self._inventory_cache.write().await;
        if cache.peek(&key).is_some() {
            return;
        }
        cache.put(key, payload);
    }
}

mod actor;
mod block_processing;
mod handle;
mod command;
mod fill_completed;
mod fill_memory_pool;
mod handlers;
mod import;
mod import_completed;
mod internal;
mod inventory_payload;
mod persist_completed;
mod relay_result;
mod reverify;
mod transaction;

pub use command::BlockchainCommand;
pub use fill_completed::FillCompleted;
pub use fill_memory_pool::FillMemoryPool;
pub use handle::BlockchainHandle;
pub use import::Import;
pub use import_completed::ImportCompleted;
pub use internal::PreverifyCompleted;
pub use inventory_payload::InventoryPayload;
pub use persist_completed::PersistCompleted;
pub use relay_result::RelayResult;
pub use reverify::{Reverify, ReverifyItem};

#[cfg(test)]
mod tests;
