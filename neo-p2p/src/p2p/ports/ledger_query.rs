//! Read-only canonical chain-state projection port.
//!
//! Replaces the session/task-manager reads that previously went through
//! `NeoSystemContext`, `LedgerContract`, and a `DataCache`/store snapshot. The
//! port names only canonical protocol payloads and primitives, so the P2P
//! layer never depends on ledger, storage, native-contract, or system types.

use neo_primitives::UInt256;
use neo_protocol::payloads::Block;

/// Read-only projections of canonical chain state.
///
/// # Fail-closed contract
///
/// Every method must answer from authoritative persisted state. An adapter that
/// cannot resolve a query returns the *absent* value (`0`, `false`, `None`, or
/// an empty `Vec`) rather than panicking or guessing. Because the sync window,
/// dedup, and forwarding decisions all compare against these answers, an absent
/// value keeps the node from advancing on unknown state instead of letting it
/// request or accept inventory it cannot verify.
pub trait LedgerQueryPort: Send + Sync {
    /// Highest persisted block index (`0` when only genesis/unknown).
    ///
    /// Unifies the `LedgerContract::current_index`, `Ledger::current_height`,
    /// and `current_block_index` reads the networking stack performed.
    fn current_index(&self) -> u32;

    /// Highest stored header index (`0` when unknown).
    ///
    /// Backs the header-request planning that combined the in-memory header
    /// cache tip with the ledger's highest header index.
    fn highest_header_index(&self) -> u32;

    /// Whether a block hash is already persisted.
    fn contains_block(&self, hash: &UInt256) -> bool;

    /// Whether a transaction hash is already persisted.
    fn contains_transaction(&self, hash: &UInt256) -> bool;

    /// Block hash at a given index, if that height is persisted.
    fn block_hash_at(&self, index: u32) -> Option<UInt256>;

    /// Up to `count` consecutive block hashes starting at `start`.
    ///
    /// Returns an empty `Vec` when `start` is unknown, which stops a
    /// `GetBlocks` reply rather than fabricating hashes.
    fn block_hashes_from(&self, start: &UInt256, count: usize) -> Vec<UInt256>;

    /// A persisted block by hash, if present.
    fn try_get_block(&self, hash: &UInt256) -> Option<Block>;
}
