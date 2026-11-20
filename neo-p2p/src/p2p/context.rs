//! System-access boundary for the P2P actor layer.
//!
//! `P2PContext` is the dependency-inversion joint that replaces the concrete
//! `NeoSystemContext` the networking actors used to hold. The trait is defined
//! here, on the P2P side, and implemented by the system runtime for its own
//! context type, so the P2P layer never names the orchestration crate it
//! serves. Every method mirrors the exact `NeoSystemContext` surface the
//! actors consume; vocabulary types are canonical protocol payloads,
//! primitives, ledger handles, or the actor runtime — never a system type.

use crate::p2p::TaskManagerHandle;
use crate::p2p::payloads::{
    extensible_payload::ExtensiblePayload, header::Header, transaction::Transaction,
};
use async_trait::async_trait;
use neo_actors::{ActorRef, ActorRuntimeResult, EventStreamHandle};
use neo_ledger::BlockchainHandle;
use neo_primitives::contains_transaction_type::ContainsTransactionType;
use neo_primitives::{UInt160, UInt256};

/// Read-only system facilities plus actor handles required by the P2P layer.
///
/// Implementations must be cheap to call from actor mailbox handlers: the
/// underlying `NeoSystemContext` accessors are lock-free reads or handle
/// clones. The trait is object-safe so actors can hold `Arc<dyn P2PContext>`.
#[async_trait]
pub trait P2PContext: Send + Sync + std::fmt::Debug {
    /// Returns the actor event stream handle for persistence subscriptions.
    fn event_stream(&self) -> EventStreamHandle;

    /// Returns the blockchain actor handle (inventory/header forwarding).
    fn blockchain(&self) -> BlockchainHandle;

    /// Returns the task manager actor handle (sync-task commands).
    fn task_manager(&self) -> TaskManagerHandle;

    /// Returns the current memory pool capacity (sizing dedup caches).
    fn mempool_capacity(&self) -> usize;

    /// Returns true when the node runs in fast-sync mode.
    fn is_fast_sync_mode(&self) -> bool;

    /// Returns the current persisted block index (height).
    fn current_block_index(&self) -> u32;

    /// Returns the highest header index persisted to the underlying store.
    fn highest_header_index(&self) -> u32;

    /// Returns the highest header index held by the in-memory header cache.
    ///
    /// `None` while the cache is still empty; callers combine this with
    /// [`Self::highest_header_index`] so a cold cache never rewinds the header
    /// request window.
    fn header_tip_index(&self) -> Option<u32>;

    /// Returns true when `hash` identifies an already-persisted block.
    fn contains_block(&self, hash: &UInt256) -> bool;

    /// Returns true when `hash` identifies a transaction already persisted in
    /// the ledger.
    ///
    /// Memory-pool presence is deliberately *not* reported: an announcement for
    /// a pooled transaction is still worth answering with `GetData`, and the
    /// task manager performs its own known-hash deduplication.
    fn contains_ledger_transaction(&self, hash: &UInt256) -> bool;

    /// Returns the stored execution block for `hash`, if present.
    fn try_get_block(&self, hash: &UInt256) -> Option<neo_ledger::Block>;

    /// Returns a persisted extensible payload for `hash`, if present.
    fn try_get_extensible(&self, hash: &UInt256) -> Option<ExtensiblePayload>;

    /// Returns a relay-cached extensible payload for `hash`, if present.
    fn try_get_relay_extensible(&self, hash: &UInt256) -> Option<ExtensiblePayload>;

    /// Returns up to `count` block hashes starting at `hash_start`.
    fn block_hashes_from(&self, hash_start: &UInt256, count: usize) -> Vec<UInt256>;

    /// Returns the block hash at `index`, if known.
    fn block_hash_at(&self, index: u32) -> Option<UInt256>;

    /// Returns up to `count` headers starting at `index_start`.
    fn headers_from_index(&self, index_start: u32, count: usize) -> Vec<Header>;

    /// Returns a mempool transaction for `hash`, if present.
    fn try_get_transaction_from_mempool(&self, hash: &UInt256) -> Option<Transaction>;

    /// Classifies where a transaction is already known (pool/chain/conflict).
    fn contains_transaction(&self, hash: &UInt256) -> ContainsTransactionType;

    /// Returns true when a conflicting transaction hash is tracked.
    fn contains_conflict_hash(&self, hash: &UInt256, signers: &[UInt160]) -> bool;

    /// Returns the hashes of all relayable mempool transactions.
    fn mempool_transaction_hashes(&self) -> Vec<UInt256>;

    /// Enqueues a transaction for state-independent preverification.
    async fn enqueue_preverify(
        &self,
        transaction: Transaction,
        relay: bool,
        sender: Option<ActorRef>,
    ) -> ActorRuntimeResult<()>;
}
