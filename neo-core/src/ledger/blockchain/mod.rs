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

use crate::akka::{Actor, ActorContext, ActorResult, Props};
use crate::error::CoreError;
use crate::events::PluginEvent;
use crate::ledger::LedgerContext;
use crate::neo_io::{MemoryReader, Serializable};
use crate::neo_system::NeoSystemContext;
use crate::network::p2p::{
    local_node::RelayInventory,
    payloads::{
        block::Block, extensible_payload::ExtensiblePayload, header::Header, InventoryType,
        Transaction,
    },
    LocalNodeCommand,
};
use crate::persistence::DataCache;
use crate::persistence::StoreCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::LedgerContract;
use crate::state_service::{StateRoot, STATE_SERVICE_CATEGORY};
use crate::{UInt160, UInt256};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use dashmap::DashMap;
use std::any::Any;
use std::collections::{HashSet, VecDeque};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{debug, warn};

use super::VerifyResult;
use types::{classify_import_block, ImportDisposition, UnverifiedBlocksList};

#[cfg(test)]
use types::should_schedule_reverify_idle;

const MAX_TX_TO_REVERIFY_PER_IDLE: usize = 10;
const MAX_REVERIFY_INVENTORY_CACHE: usize = 256;

#[derive(Debug, Clone, Copy, Hash, PartialEq, Eq)]
struct InventoryCacheKey {
    inventory_type: InventoryType,
    payload_hash: UInt256,
}

impl InventoryCacheKey {
    fn new(inventory_type: InventoryType, payload: &[u8]) -> Self {
        let payload_hash = UInt256::from(crate::cryptography::Crypto::sha256(payload));
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

/// Rust analogue of `Neo.Ledger.Blockchain` (actor based on Akka).
pub struct Blockchain {
    ledger: Arc<LedgerContext>,
    system_context: Option<Arc<NeoSystemContext>>,
    _block_cache: Arc<DashMap<UInt256, Arc<Block>>>,
    _block_cache_unverified: Arc<DashMap<u32, UnverifiedBlocksList>>,
    _extensible_witness_white_list: Arc<RwLock<HashSet<UInt160>>>,
    _inventory_cache: Arc<DashMap<InventoryCacheKey, InventoryPayload>>,
    _inventory_cache_order: Arc<RwLock<VecDeque<InventoryCacheKey>>>,
}

impl Blockchain {
    pub fn new(ledger: Arc<LedgerContext>) -> Self {
        Self {
            ledger,
            system_context: None,
            _block_cache: Arc::new(DashMap::with_capacity(1024)),
            _block_cache_unverified: Arc::new(DashMap::with_capacity(256)),
            _extensible_witness_white_list: Arc::new(RwLock::new(HashSet::new())),
            _inventory_cache: Arc::new(DashMap::with_capacity(2048)),
            _inventory_cache_order: Arc::new(RwLock::new(VecDeque::with_capacity(2048))),
        }
    }

    pub fn props(ledger: Arc<LedgerContext>) -> Props {
        Props::new(move || Self::new(ledger.clone()))
    }

    fn persist_block_via_system(&self, block: &Arc<Block>) -> bool {
        let Some(context) = &self.system_context else {
            return false;
        };

        let Some(system) = context.neo_system() else {
            return false;
        };

        let hash = block.header.clone().hash();

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

    fn inventory_cache_get(&self, key: &InventoryCacheKey) -> Option<InventoryPayload> {
        self._inventory_cache.get(key).map(|r| r.clone())
    }

    async fn inventory_cache_insert(&self, key: InventoryCacheKey, payload: InventoryPayload) {
        if self._inventory_cache.contains_key(&key) {
            return;
        }

        let mut order = self._inventory_cache_order.write().await;
        self._inventory_cache.insert(key, payload);
        order.push_back(key);

        while order.len() > MAX_REVERIFY_INVENTORY_CACHE {
            if let Some(evicted) = order.pop_front() {
                self._inventory_cache.remove(&evicted);
            }
        }
    }
}

mod actor;
mod block_processing;
mod handlers;
mod transaction;
mod types;

pub use types::{
    BlockchainCommand, FillCompleted, FillMemoryPool, Import, ImportCompleted, InventoryPayload,
    PersistCompleted, PreverifyCompleted, RelayResult, Reverify, ReverifyItem,
};

#[cfg(test)]
mod tests;
