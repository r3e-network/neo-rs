//! Blockchain actor: message handling over ledger-owned block policy.
//!
//! Every decision this actor makes *about blocks* is owned by `neo-ledger`:
//!
//! - [`neo_ledger::admit_block_for_persistence`] decides whether a block may be
//!   persisted, reading the chain state through
//!   [`neo_ledger::BlockAdmissionPort`], which the system context implements
//!   below;
//! - [`neo_ledger::BlockCachePolicy`] owns the verified and unverified block
//!   caches, their shared byte budget and every retention, eviction and
//!   displacement decision.
//!
//! What is left here is orchestration: the actor loop, the system context it
//! borrows, the extensible whitelist and inventory bookkeeping, the persist and
//! relay plumbing, and the local-failure counter the inventory paths need to
//! tell a local storage failure from an invalid block.
//!
//! # Block cache budget
//!
//! The two block caches charge one budget shared with the block body cache of
//! [`LedgerContext`], so the three together never retain more bytes than its
//! limit. When a block does not fit, room is made inside the policy; the actor
//! only asks whether a block was retained and reports the verdict to the peer
//! that sent it.

use crate::events::PluginEvent;
use crate::neo_system::NeoSystemContext;
use crate::network::p2p::{
    local_node::RelayInventory,
    payloads::{InventoryType, Transaction, extensible_payload::ExtensiblePayload},
};
use crate::state_service::{STATE_SERVICE_CATEGORY, StateRoot};
use async_trait::async_trait;
use lru::LruCache;
use neo_actors::{Actor, ActorContext, ActorResult, Props, ScheduleHandle};
use neo_io_crate::{MemoryReader, Serializable};
use neo_ledger::{
    BlockAdmissionPort, BlockCachePolicy, LedgerContext, MAX_BLOCK_CACHE_SIZE,
    MAX_UNVERIFIED_CACHE_BYTES, PersistenceAdmission,
};
use neo_native_contracts::LedgerContract;
use neo_primitives::{UInt160, UInt256};
use neo_protocol::execution::Block;
use neo_protocol::payloads::{Block as PayloadBlock, Header};
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_smart_contract::error::CoreError;
use neo_smart_contract::error::CoreResult;
use neo_storage::persistence::DataCache;
use neo_storage::persistence::StoreCache;
use std::any::Any;
use std::collections::HashSet;
use std::num::NonZeroUsize;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
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

// Beside a full unverified cache the default budget must still have room for
// the largest block a peer can send and for the body the ledger context keeps
// of it once it is persisted. The ledger owns the budget and the cache caps;
// only this crate knows how large a peer payload may be, so the check lives
// here.
const _: () = assert!(
    neo_ledger::DEFAULT_BLOCK_CACHE_BUDGET_BYTES
        >= MAX_UNVERIFIED_CACHE_BYTES + 2 * crate::network::p2p::PAYLOAD_MAX_SIZE
);

/// Rust analogue of `Neo.Ledger.Blockchain` using the async actor runtime.
pub struct Blockchain {
    ledger: Arc<LedgerContext>,
    system_context: Option<Arc<NeoSystemContext>>,
    /// Ledger-owned retention policy of the verified and unverified caches.
    cache: BlockCachePolicy,
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
            cache: BlockCachePolicy::new(block_cache_budget),
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

    /// Verifies a block immediately before it is persisted.
    ///
    /// The admission policy itself (next-height check, body integrity, the
    /// cached-header fast path and the choice between cached and store-backed
    /// header verification) is owned by `neo_ledger::admit_block_for_persistence`;
    /// the actor only logs the verdict and reports it.
    fn verify_block_for_persistence(&self, block: &Block, context: &NeoSystemContext) -> bool {
        let verdict = neo_ledger::admit_block_for_persistence(block, context);
        if verdict.is_accepted() {
            return true;
        }

        let block_hash = block.hash();
        match verdict {
            PersistenceAdmission::NotNextHeight { current_height } => warn!(
                target: "neo",
                index = block.index(),
                current_height,
                "refusing to persist block that is not the next height"
            ),
            PersistenceAdmission::BodyIntegrityFailed => warn!(
                target: "neo",
                index = block.index(),
                %block_hash,
                "refusing to persist block that failed body integrity check"
            ),
            PersistenceAdmission::HeaderHashMismatch => warn!(
                target: "neo",
                index = block.index(),
                %block_hash,
                "refusing to persist block whose hash differs from the verified cached header"
            ),
            PersistenceAdmission::CachedHeaderHashFailed => warn!(
                target: "neo",
                index = block.index(),
                error = "cached header hash failed",
                "refusing to persist block: cached header hash failed"
            ),
            PersistenceAdmission::VerificationFailed => warn!(
                target: "neo",
                index = block.index(),
                %block_hash,
                "refusing to persist block that failed verification"
            ),
            PersistenceAdmission::Accept => {}
        }
        false
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

/// The node's chain state, as the ledger admission policy asks for it.
///
/// The policy decides *whether* a block may be persisted; this only answers
/// what the policy asks: the persisted height, the verified headers, and the
/// two ways a header is verified. Nothing here decides anything itself.
impl BlockAdmissionPort for NeoSystemContext {
    fn persisted_height(&self) -> u32 {
        self.ledger().current_height()
    }

    fn header_at(&self, index: u32) -> Option<Header> {
        self.header_cache().get(index)
    }

    fn last_header_height(&self) -> Option<u32> {
        self.header_cache().last().map(|header| header.index())
    }

    fn verify_against_cached_headers(&self, block: &PayloadBlock) -> bool {
        neo_ledger::verify_block_with_cache(
            block,
            self.settings().as_ref(),
            &self.store_cache(),
            &self.header_cache(),
        )
    }

    fn verify_against_store(&self, block: &PayloadBlock) -> bool {
        neo_ledger::verify_block(block, self.settings().as_ref(), &self.store_cache())
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
