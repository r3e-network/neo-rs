//! Admission policy for a block that is about to be persisted.
//!
//! The blockchain actor parks blocks in caches before they are committed: a
//! verified block waits in the block cache, and a block whose predecessor is
//! still unknown waits in the unverified cache without being verified at all.
//! Neither cache makes a block trustworthy, so every block is admitted again
//! immediately before persistence:
//!
//! 1. it must be the next height on the chain;
//! 2. its body must match the transactions it claims (a header hash commits to
//!    a merkle root, not to the supplied transactions);
//! 3. if a header for that height was already verified, the block hash must
//!    equal that header's hash — that is the fast path;
//! 4. otherwise the header is verified, against the header cache when the block
//!    extends it, or against the store when it does not.
//!
//! These are ledger-domain rules selected from ledger state, so they live here;
//! the actor only logs the verdict and decides what to do with a rejected block.
//!
//! The ledger state the rules read is reached through [`BlockAdmissionPort`], so
//! this module names no cache, store or system type: the system layer implements
//! the port over whatever it holds and gets a verdict back.

use neo_protocol::execution::Block;
use neo_protocol::payloads::{Block as PayloadBlock, Header};

/// Why a block may not be persisted, or [`PersistenceAdmission::Accept`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PersistenceAdmission {
    /// The block passed every admission check and may be persisted.
    Accept,
    /// The block is not the next height on the chain.
    NotNextHeight {
        /// Height the ledger is at.
        current_height: u32,
    },
    /// The block body does not match the transactions it claims.
    BodyIntegrityFailed,
    /// The block hash differs from the already-verified cached header.
    HeaderHashMismatch,
    /// The hash of the cached header could not be computed.
    CachedHeaderHashFailed,
    /// Header verification failed.
    VerificationFailed,
}

impl PersistenceAdmission {
    /// Returns true only for [`PersistenceAdmission::Accept`].
    #[must_use]
    pub fn is_accepted(self) -> bool {
        matches!(self, Self::Accept)
    }
}

/// The ledger state a persistence admission reads, as narrow queries.
///
/// Implemented by the party that holds the chain state (the node's system
/// context); the policy above never sees a concrete header or store cache.
pub trait BlockAdmissionPort {
    /// Height of the persisted chain tip.
    fn persisted_height(&self) -> u32;

    /// Verified header for `index`, if the node has one.
    fn header_at(&self, index: u32) -> Option<Header>;

    /// Height of the highest verified header, if any.
    fn last_header_height(&self) -> Option<u32>;

    /// Verifies `block`'s header against the verified header chain.
    fn verify_against_cached_headers(&self, block: &PayloadBlock) -> bool;

    /// Verifies `block`'s header against the persisted store.
    fn verify_against_store(&self, block: &PayloadBlock) -> bool;
}

/// Decides whether `block` may be persisted, reading the chain state it needs
/// through `port`.
#[must_use]
pub fn admit_block_for_persistence(
    block: &Block,
    port: &dyn BlockAdmissionPort,
) -> PersistenceAdmission {
    let current_height = port.persisted_height();
    if block.index() != current_height.saturating_add(1) {
        return PersistenceAdmission::NotNextHeight { current_height };
    }

    let block_hash = block.hash();

    // Body integrity first, unconditionally. Ingress already checks it, so this
    // is defence in depth for the fast path below: a header hash commits to the
    // merkle root, not to the transactions actually supplied, so a matching
    // hash alone says nothing about the body.
    let Ok(payload_block) = crate::verify_execution_block_integrity(block) else {
        return PersistenceAdmission::BodyIntegrityFailed;
    };

    // Fast path: this height was already validated when its header was accepted
    // (header sync / contiguous block receive). Hash equality proves the header
    // is the verified one; together with the integrity check above, the body is
    // the one that header's merkle root commits to.
    if let Some(mut header) = port.header_at(block.index())
        && header.index() == block.index()
    {
        return match header.try_hash() {
            Ok(header_hash) if header_hash == block_hash => PersistenceAdmission::Accept,
            Ok(_) => PersistenceAdmission::HeaderHashMismatch,
            Err(_) => PersistenceAdmission::CachedHeaderHashFailed,
        };
    }

    let header_height = port.last_header_height().unwrap_or(current_height);
    let verified = if block.index() == header_height.saturating_add(1) {
        port.verify_against_cached_headers(&payload_block)
    } else {
        port.verify_against_store(&payload_block)
    };

    if verified {
        PersistenceAdmission::Accept
    } else {
        PersistenceAdmission::VerificationFailed
    }
}
