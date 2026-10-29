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
use crate::neo_system::NeoSystemContext;
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
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::RwLock;
use tracing::{debug, warn};

use super::VerifyResult;
use types::{ImportDisposition, UnverifiedBlocksList, classify_import_block};

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

/// Maximum number of blocks retained in the unverified block cache.
/// This is a block count, not a height count: a malicious peer can otherwise
/// fill one height with arbitrarily many competing blocks.
const MAX_UNVERIFIED_CACHE_SIZE: usize = 20_000;

/// Maximum number of competing blocks retained for a single height.
///
/// The consensus protocol can legitimately produce a small number of fork
/// candidates at the same height. Keeping more than this is only useful to an
/// attacker trying to exhaust memory with unsolicited blocks, because none of
/// them can be persisted until their height is verified against a cached
/// header.
const MAX_UNVERIFIED_CANDIDATES_PER_HEIGHT: usize = 3;

/// Hard cap on the approximate serialized size retained by the unverified
/// block cache. The entry/candidate limits above are not sufficient on their
/// own: a P2P payload may be up to 32 MiB, so 20k cached blocks could pin
/// hundreds of gigabytes of RAM. 256 MiB covers the fast-sync window while
/// bounding worst-case memory under byzantine peers.
const MAX_UNVERIFIED_CACHE_BYTES: usize = 256 * 1024 * 1024;

/// Per-entry accounting overhead (hash map/list bookkeeping), used to keep the
/// byte budget honest for very small blocks.
const UNVERIFIED_BLOCK_OVERHEAD: usize = 64;

/// Rust analogue of `Neo.Ledger.Blockchain` using the async actor runtime.
pub struct Blockchain {
    ledger: Arc<LedgerContext>,
    system_context: Option<Arc<NeoSystemContext>>,
    _block_cache: Arc<DashMap<UInt256, Arc<Block>>>,
    _block_cache_unverified: Arc<DashMap<u32, UnverifiedBlocksList>>,
    /// Total approximate serialized bytes currently held in
    /// `_block_cache_unverified`. Guarded by atomics because the cache is
    /// shared through `Arc` even though the actor processes messages serially.
    _unverified_cache_bytes: Arc<AtomicUsize>,
    /// Total number of blocks currently held in `_block_cache_unverified`.
    _unverified_cache_blocks: Arc<AtomicUsize>,
    _extensible_witness_white_list: Arc<RwLock<HashSet<UInt160>>>,
    _inventory_cache: Arc<RwLock<LruCache<InventoryCacheKey, InventoryPayload>>>,
    _drain_timer: Option<ScheduleHandle>,
}

impl Blockchain {
    /// Creates the blockchain actor over the given ledger context.
    pub fn new(ledger: Arc<LedgerContext>) -> Self {
        Self {
            ledger,
            system_context: None,
            _block_cache: Arc::new(DashMap::with_capacity(1024)),
            _block_cache_unverified: Arc::new(DashMap::with_capacity(256)),
            _unverified_cache_bytes: Arc::new(AtomicUsize::new(0)),
            _unverified_cache_blocks: Arc::new(AtomicUsize::new(0)),
            _extensible_witness_white_list: Arc::new(RwLock::new(HashSet::new())),
            _inventory_cache: Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(MAX_REVERIFY_INVENTORY_CACHE)
                    .expect("inventory cache capacity is non-zero"),
            ))),
            _drain_timer: None,
        }
    }

    /// Builds actor `Props` that spawn a `Blockchain` over `ledger`.
    pub fn props(ledger: Arc<LedgerContext>) -> Props {
        Props::new(move || Self::new(ledger.clone()))
    }

    fn try_block_hash(block: &Block) -> CoreResult<UInt256> {
        let mut header = block.header.clone();
        header.try_hash()
    }

    /// Approximate memory cost of one cached unverified block.
    fn unverified_block_cost(block: &Block) -> usize {
        block.size().saturating_add(UNVERIFIED_BLOCK_OVERHEAD)
    }

    /// Removes every unverified candidate at `index`, keeping the cache byte
    /// and block counters consistent.
    fn remove_unverified_index(&self, index: u32) -> Option<UnverifiedBlocksList> {
        let removed = self
            ._block_cache_unverified
            .remove(&index)
            .map(|(_, list)| list)?;
        let bytes = removed
            .blocks
            .iter()
            .map(|block| Self::unverified_block_cost(block.as_ref()))
            .fold(0usize, usize::saturating_add);
        let count = removed.blocks.len();
        self._unverified_cache_bytes
            .update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.saturating_sub(bytes)
            });
        self._unverified_cache_blocks
            .update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.saturating_sub(count)
            });
        Some(removed)
    }

    /// Verifies a block immediately before it is persisted.
    ///
    /// Blocks parked in the unverified cache were deliberately not verified
    /// when they arrived (their predecessor was not yet known). Before they are
    /// persisted we must either match them against a header that was already
    /// verified, or run full header verification (index/prev-hash/timestamp/
    /// witness). Without this check any peer can inject an arbitrary block for
    /// a future height and have it committed once the chain catches up.
    fn verify_block_for_persistence(&self, block: &Block, context: &NeoSystemContext) -> bool {
        let current_height = context.ledger().current_height();
        if block.index() != current_height.saturating_add(1) {
            warn!(
                target: "neo",
                index = block.index(),
                current_height,
                "refusing to persist block that is not the next height"
            );
            return false;
        }

        let settings = context.settings();
        let store_cache = context.store_cache();
        let header_cache = context.header_cache();
        let block_hash = match Self::try_block_hash(block) {
            Ok(hash) => hash,
            Err(error) => {
                warn!(
                    target: "neo",
                    index = block.index(),
                    error = %error,
                    "refusing to persist block with uncomputable hash"
                );
                return false;
            }
        };

        // Fast path: this height was already validated when its header was
        // accepted (header sync / contiguous block receive). The block hash
        // commits to the merkle root, so hash equality proves the transaction
        // set matches the verified header.
        if let Some(mut header) = header_cache.get(block.index())
            && header.index() == block.index()
        {
            return match header.try_hash() {
                Ok(header_hash) if header_hash == block_hash => true,
                Ok(_) => {
                    warn!(
                        target: "neo",
                        index = block.index(),
                        %block_hash,
                        "refusing to persist block whose hash differs from the verified cached header"
                    );
                    false
                }
                Err(error) => {
                    warn!(
                        target: "neo",
                        index = block.index(),
                        error = %error,
                        "refusing to persist block: cached header hash failed"
                    );
                    false
                }
            };
        }

        let header_height = header_cache
            .last()
            .map(|header| header.index())
            .unwrap_or(current_height);
        let verified = if block.index() == header_height.saturating_add(1) {
            block.verify_with_cache(settings.as_ref(), &store_cache, &header_cache)
        } else {
            block.verify(settings.as_ref(), &store_cache)
        };
        if !verified {
            warn!(
                target: "neo",
                index = block.index(),
                %block_hash,
                "refusing to persist block that failed verification"
            );
        }
        verified
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
mod handlers;
mod transaction;
mod types;

pub use handle::BlockchainHandle;
pub use types::{
    BlockchainCommand, FillCompleted, FillMemoryPool, Import, ImportCompleted, InventoryPayload,
    PersistCompleted, PreverifyCompleted, RelayResult, Reverify, ReverifyItem,
};

#[cfg(test)]
mod tests;
