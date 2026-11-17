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
//!
//! # Block Cache Budget
//!
//! Both block caches charge every block they hold to the
//! [`BlockCacheBudget`] they share with the block body cache of
//! [`LedgerContext`], so the three together never retain more bytes than its
//! limit. A block held by both actor caches is charged once.
//!
//! The caches only save a download: every block taken from them is verified
//! again before it is persisted, and a block that is not cached is fetched
//! again. Giving up a cached block therefore never changes what is valid or
//! the order in which blocks are persisted.
//!
//! When a block does not fit, room is made in this order, and admission is
//! refused if that is not enough:
//!
//! 1. cached blocks at or below the persisted height, which are of no use;
//! 2. persisted block bodies in the ledger context, oldest first, which the
//!    store can still serve;
//! 3. cached blocks at heights above the incoming block, highest first.
//!
//! The budget never makes a block give way to one at the same or a greater
//! height. The chain needs the lowest missing height next, and blocks nobody
//! can verify yet sit above every height a verified header exists for, so
//! filling the budget with far-future blocks cannot keep a block the chain
//! needs sooner out.
//!
//! The next block to persist does not depend on admission at all: it is
//! persisted even when the budget has no room for it or it is larger than the
//! whole budget, it is just not retained, and nothing is given up for it. Any
//! other block that cannot be admitted is not kept, and is reported as
//! `OutOfMemory` if it was verified.

use crate::events::PluginEvent;
use crate::neo_system::NeoSystemContext;
use crate::network::p2p::{
    local_node::RelayInventory,
    payloads::{InventoryType, Transaction, extensible_payload::ExtensiblePayload},
};
use crate::state_service::{STATE_SERVICE_CATEGORY, StateRoot};
use async_trait::async_trait;
use dashmap::DashMap;
use lru::LruCache;
use neo_actors::{Actor, ActorContext, ActorResult, Props, ScheduleHandle};
use neo_io_crate::{MemoryReader, Serializable};
use neo_ledger::{
    BlockCacheBudget, BlockCacheReservation, LedgerContext, execution_block_cache_cost,
};
use neo_native_contracts::LedgerContract;
use neo_primitives::{UInt160, UInt256};
use neo_protocol::execution::Block;
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_smart_contract::error::CoreError;
use neo_smart_contract::error::CoreResult;
use neo_storage::persistence::DataCache;
use neo_storage::persistence::StoreCache;
use std::any::Any;
use std::collections::{BTreeMap, HashSet};
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use tokio::sync::RwLock;
use tracing::{debug, warn};

use neo_ledger::blockchain::types::{ImportDisposition, classify_import_block};
use neo_primitives::VerifyResult;

/// System-attachment command accepted by the blockchain actor.
///
/// Split from `BlockchainCommand` (which stays in `neo-core` so the wider
/// workspace can keep sending it without depending on this crate) because
/// the `AttachSystem` payload carries [`NeoSystemContext`], which now lives
/// here. The actor downcasts this envelope after `BlockchainCommand`.
#[derive(Debug, Clone)]
pub enum BlockchainSystemCommand {
    /// Attach the system context.
    AttachSystem(Arc<NeoSystemContext>),
}

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
///
/// This is a share of the block cache budget, not an addition to it: blocks
/// in the unverified cache are charged to the budget as well.
const MAX_UNVERIFIED_CACHE_BYTES: usize = 256 * 1024 * 1024;

// Beside a full unverified cache the default budget must still have room for
// the largest block a peer can send and for the body the ledger context keeps
// of it once it is persisted.
const _: () = assert!(
    neo_ledger::DEFAULT_BLOCK_CACHE_BUDGET_BYTES
        >= MAX_UNVERIFIED_CACHE_BYTES + 2 * crate::network::p2p::PAYLOAD_MAX_SIZE
);

/// A block held by the verified block cache, the unverified block cache, or
/// both, together with the bytes it holds in the block cache budget.
///
/// A verified block that cannot be persisted yet sits in both caches. The two
/// entries are clones sharing one reservation, so the block is charged once
/// and its bytes are returned when the last of them is dropped.
#[derive(Clone)]
struct RetainedBlock {
    // Declared before the reservation so the block is released before its
    // bytes are returned to the budget.
    block: Arc<Block>,
    hash: UInt256,
    /// Budget cost of the block, computed once when it was admitted.
    cost: usize,
    _reservation: Arc<BlockCacheReservation>,
}

/// Rust analogue of `Neo.Ledger.Blockchain` using the async actor runtime.
pub struct Blockchain {
    ledger: Arc<LedgerContext>,
    system_context: Option<Arc<NeoSystemContext>>,
    _block_cache: Arc<DashMap<UInt256, RetainedBlock>>,
    _block_cache_unverified: Arc<DashMap<u32, Vec<RetainedBlock>>>,
    /// Byte budget both block caches share with the block body cache of
    /// `ledger`.
    _block_cache_budget: Arc<BlockCacheBudget>,
    /// Total budget cost of the blocks currently held in
    /// `_block_cache_unverified`. Guarded by atomics because the cache is
    /// shared through `Arc` even though the actor processes messages serially.
    _unverified_cache_bytes: Arc<AtomicUsize>,
    /// Total number of blocks currently held in `_block_cache_unverified`.
    _unverified_cache_blocks: Arc<AtomicUsize>,
    _extensible_witness_white_list: Arc<RwLock<HashSet<UInt160>>>,
    _inventory_cache: Arc<RwLock<LruCache<InventoryCacheKey, InventoryPayload>>>,
    /// Number of times a local persistence attempt failed since actor start.
    ///
    /// `on_new_block` collapses "the block is invalid" and "local storage could
    /// not commit" into `VerifyResult::Invalid`. Inventory handlers sample this
    /// counter around the call so a failure that advanced it is reported as a
    /// transient local failure instead of an invalid-block verdict.
    _local_persist_failures: Arc<AtomicU64>,
    _drain_timer: Option<ScheduleHandle>,
}

impl Blockchain {
    /// Creates the blockchain actor over the given ledger context.
    pub fn new(ledger: Arc<LedgerContext>) -> Self {
        let block_cache_budget = ledger.block_cache_budget();
        Self {
            ledger,
            system_context: None,
            _block_cache: Arc::new(DashMap::with_capacity(1024)),
            _block_cache_unverified: Arc::new(DashMap::with_capacity(256)),
            _block_cache_budget: block_cache_budget,
            _unverified_cache_bytes: Arc::new(AtomicUsize::new(0)),
            _unverified_cache_blocks: Arc::new(AtomicUsize::new(0)),
            _extensible_witness_white_list: Arc::new(RwLock::new(HashSet::new())),
            _inventory_cache: Arc::new(RwLock::new(LruCache::new(
                NonZeroUsize::new(MAX_REVERIFY_INVENTORY_CACHE)
                    .expect("inventory cache capacity is non-zero"),
            ))),
            _local_persist_failures: Arc::new(AtomicU64::new(0)),
            _drain_timer: None,
        }
    }

    /// Builds actor `Props` that spawn a `Blockchain` over `ledger`.
    pub fn props(ledger: Arc<LedgerContext>) -> Props {
        Props::new(move || Self::new(ledger.clone()))
    }

    fn try_block_hash(block: &Block) -> CoreResult<UInt256> {
        Ok(block.hash())
    }

    /// Charges `block` to the block cache budget if it fits as things stand,
    /// without making room for it.
    fn try_retain_block(&self, block: Arc<Block>, hash: UInt256) -> Option<RetainedBlock> {
        let cost = execution_block_cache_cost(block.as_ref());
        let reservation = self._block_cache_budget.try_reserve(cost)?;
        Some(RetainedBlock {
            block,
            hash,
            cost,
            _reservation: Arc::new(reservation),
        })
    }

    /// Charges `block` to the block cache budget, making room for it as
    /// described in the module documentation. Returns `None` when the block
    /// must not be retained.
    fn retain_block(&self, block: Arc<Block>, hash: UInt256) -> Option<RetainedBlock> {
        let cost = execution_block_cache_cost(block.as_ref());
        let reservation = self.reserve_block_bytes(block.index(), cost)?;
        Some(RetainedBlock {
            block,
            hash,
            cost,
            _reservation: Arc::new(reservation),
        })
    }

    /// Reserves `cost` bytes for a block at height `index`, giving up cached
    /// blocks of lower priority when the budget is short.
    fn reserve_block_bytes(&self, index: u32, cost: usize) -> Option<BlockCacheReservation> {
        let budget = &self._block_cache_budget;
        if let Some(reservation) = budget.try_reserve(cost) {
            return Some(reservation);
        }
        if cost > budget.limit() {
            // Nothing that could be given up would make this block fit.
            return None;
        }

        self.evict_blocks_up_to(self.ledger.current_height());
        if let Some(reservation) = budget.try_reserve(cost) {
            return Some(reservation);
        }

        self.ledger
            .evict_block_bodies(cost.saturating_sub(budget.available()));
        if let Some(reservation) = budget.try_reserve(cost) {
            return Some(reservation);
        }

        for (height, hashes) in self.cached_heights_above(index).into_iter().rev() {
            self.remove_unverified_index(height);
            for hash in hashes {
                self._block_cache.remove(&hash);
            }
            if let Some(reservation) = budget.try_reserve(cost) {
                return Some(reservation);
            }
        }
        None
    }

    /// Drops every cached block at or below `height`. Those are persisted
    /// already, or lost to the block that was, and are never looked up again.
    fn evict_blocks_up_to(&self, height: u32) {
        let stale: Vec<UInt256> = self
            ._block_cache
            .iter()
            .filter(|entry| entry.value().block.index() <= height)
            .map(|entry| *entry.key())
            .collect();
        for hash in stale {
            self._block_cache.remove(&hash);
        }
        let stale: Vec<u32> = self
            ._block_cache_unverified
            .iter()
            .map(|entry| *entry.key())
            .filter(|index| *index <= height)
            .collect();
        for index in stale {
            self.remove_unverified_index(index);
        }
    }

    /// Heights above `index` at which either cache holds a block, each with
    /// the hashes the verified cache holds there.
    fn cached_heights_above(&self, index: u32) -> BTreeMap<u32, Vec<UInt256>> {
        let mut heights: BTreeMap<u32, Vec<UInt256>> = BTreeMap::new();
        for entry in self._block_cache_unverified.iter() {
            if *entry.key() > index {
                heights.entry(*entry.key()).or_default();
            }
        }
        for entry in self._block_cache.iter() {
            let height = entry.value().block.index();
            if height > index {
                heights.entry(height).or_default().push(*entry.key());
            }
        }
        heights
    }

    /// Takes `count` blocks costing `bytes` off the unverified cache counters.
    fn release_unverified(&self, bytes: usize, count: usize) {
        self._unverified_cache_bytes
            .update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.saturating_sub(bytes)
            });
        self._unverified_cache_blocks
            .update(Ordering::Relaxed, Ordering::Relaxed, |current| {
                current.saturating_sub(count)
            });
    }

    /// Removes every unverified candidate at `index`, keeping the cache byte
    /// and block counters consistent.
    fn remove_unverified_index(&self, index: u32) -> Option<Vec<RetainedBlock>> {
        let removed = self
            ._block_cache_unverified
            .remove(&index)
            .map(|(_, list)| list)?;
        let bytes = removed
            .iter()
            .map(|entry| entry.cost)
            .fold(0usize, usize::saturating_add);
        self.release_unverified(bytes, removed.len());
        Some(removed)
    }

    /// Verifies a block immediately before it is persisted.
    ///
    /// Blocks parked in the unverified cache were deliberately not verified
    /// when they arrived (their predecessor was not yet known). Before they are
    /// persisted we re-check body integrity, then either match them against a
    /// header that was already verified, or run full header verification
    /// (index/prev-hash/timestamp/witness). Without this check any peer can
    /// inject an arbitrary block for a future height and have it committed
    /// once the chain catches up.
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

        // Body integrity first, unconditionally. Ingress already checks it, so
        // this is defence in depth for the fast path below: a header hash
        // commits to the merkle root, not to the transactions actually
        // supplied, so a matching hash alone says nothing about the body.
        let payload_block = match neo_ledger::verify_execution_block_integrity(block) {
            Ok(payload_block) => payload_block,
            Err(error) => {
                warn!(
                    target: "neo",
                    index = block.index(),
                    %block_hash,
                    %error,
                    "refusing to persist block that failed body integrity check"
                );
                return false;
            }
        };

        // Fast path: this height was already validated when its header was
        // accepted (header sync / contiguous block receive). Hash equality
        // proves the header is the verified one; together with the integrity
        // check above, the body is the one that header's merkle root commits to.
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
            .map_or(current_height, |header| header.index());
        let verified = if block.index() == header_height.saturating_add(1) {
            neo_ledger::verify_block_with_cache(
                &payload_block,
                settings.as_ref(),
                &store_cache,
                &header_cache,
            )
        } else {
            neo_ledger::verify_block(&payload_block, settings.as_ref(), &store_cache)
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
            Ok(()) => {
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
                // Record that the failure was local (storage/pipeline), not a
                // verdict about the block itself. Inventory handlers use this to
                // report a transient local failure to the peer-facing caller.
                self._local_persist_failures.fetch_add(1, Ordering::Relaxed);
                // Let the task manager release this height for a retry. This is
                // a local commit failure, not evidence that the peer is invalid.
                context.actor_system.event_stream().publish(PersistFailed {
                    block: Arc::clone(block),
                    error: error.clone(),
                });
                false
            }
        }
    }

    /// Snapshot of the local persistence-failure counter.
    ///
    /// Callers sample this before triggering persistence and compare afterwards:
    /// an unchanged value means a persistence-rejecting verdict came from block
    /// validation, not from local storage.
    fn local_persist_failures(&self) -> u64 {
        self._local_persist_failures.load(Ordering::Relaxed)
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
mod handlers;
mod transaction;

// Re-export types from neo-ledger
pub use neo_ledger::blockchain::{
    BlockchainCommand, BlockchainHandle, FillCompleted, FillMemoryPool, Import, ImportCompleted,
    ImportError, ImportResult, InventoryPayload, PersistCompleted, PersistFailed,
    PreverifyCompleted, RelayResult, Reverify, ReverifyItem,
};

#[cfg(test)]
mod block_cache_budget_tests;
#[cfg(test)]
mod import_integrity_tests;
#[cfg(test)]
mod tests;
