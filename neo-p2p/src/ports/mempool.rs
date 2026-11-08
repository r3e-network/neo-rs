//! In-memory relayable-inventory query port (mempool + relay cache).
//!
//! Replaces the session/task-manager reads that previously went through
//! `NeoSystemContext` into the memory pool and the in-memory relay/extensible
//! caches. Only canonical protocol payloads and primitives cross the boundary.

use crate::ContainsTransactionType;
use neo_primitives::{UInt160, UInt256};
use neo_protocol::payloads::{Extensible, Transaction};

/// Read-only projections of in-memory relayable inventory.
///
/// This covers both the transaction memory pool and the short-lived relay cache
/// that serves `GetData` for extensible payloads (dBFT / state-service items).
/// They share one port because both answer "what can this node relay right now"
/// without touching persisted chain state.
///
/// # Fail-closed contract
///
/// An adapter that cannot answer returns the absent value (`0`, empty, `None`,
/// or [`ContainsTransactionType::NotExist`]) rather than panicking. Serving a
/// `GetData` or relay decision from an absent answer simply declines to relay,
/// which is always safe; claiming inventory exists when it cannot be produced
/// is not.
pub trait MempoolQueryPort: Send + Sync {
    /// Configured memory-pool capacity.
    ///
    /// Used to size the task manager's known-hash dedup cache; the caller
    /// clamps it to a sane minimum, so `0` degrades gracefully.
    fn capacity(&self) -> usize;

    /// Hashes currently held in the memory pool.
    ///
    /// Backs the `Mempool` request reply; an empty list sends nothing.
    fn transaction_hashes(&self) -> Vec<UInt256>;

    /// A memory-pool transaction by hash, if present.
    fn try_get_transaction(&self, hash: &UInt256) -> Option<Transaction>;

    /// Relay containment status for a transaction hash.
    ///
    /// Distinguishes already-persisted / in-pool / conflicting / unknown so the
    /// session can skip re-relaying or re-verifying a known transaction.
    fn contains_transaction(&self, hash: &UInt256) -> ContainsTransactionType;

    /// Whether the hash conflicts with an in-memory signer account.
    ///
    /// A conflict means the transaction must not be accepted for relay even if
    /// it is otherwise unknown.
    fn contains_conflict_hash(&self, hash: &UInt256, signers: &[UInt160]) -> bool;

    /// A relayable extensible payload by hash, if present.
    fn try_get_relay_extensible(&self, hash: &UInt256) -> Option<Extensible>;

    /// A cached extensible payload by hash, if present.
    fn try_get_extensible(&self, hash: &UInt256) -> Option<Extensible>;
}
