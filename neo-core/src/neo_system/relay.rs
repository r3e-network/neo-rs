//! Relay cache types for extensible payload management.
//!
//! This module provides types for caching and managing extensible payloads
//! during network relay operations.

use crate::network::p2p::payloads::extensible_payload::ExtensiblePayload;
use neo_primitives::UInt256;
use neo_io_crate::{InventoryHash, RelayCache};

/// Default capacity for the relay extensible cache.
pub(crate) const RELAY_CACHE_CAPACITY: usize = 100;

/// Maximum number of historical blocks/headers to hydrate into memory on startup.
/// Older data remains accessible via the store but is not preloaded to reduce memory use.
pub(crate) const LEDGER_HYDRATION_WINDOW: u32 = 2_000;

/// Entry in the relay cache for extensible payloads.
///
/// Wraps an `ExtensiblePayload` with its precomputed hash for efficient
/// lookup and deduplication during network relay operations.
#[derive(Clone)]
pub(crate) struct RelayExtensibleEntry {
    hash: UInt256,
    payload: ExtensiblePayload,
}

impl RelayExtensibleEntry {
    /// Creates a new relay entry from an extensible payload.
    ///
    /// The hash is computed once during construction for efficient lookups.
    pub(crate) fn new(mut payload: ExtensiblePayload) -> Self {
        let hash = payload.hash();
        Self { hash, payload }
    }

    /// Returns a clone of the wrapped payload.
    pub(crate) fn payload(&self) -> ExtensiblePayload {
        self.payload.clone()
    }

    /// Returns the precomputed hash of the payload.
    #[allow(dead_code)]
    pub(crate) fn hash(&self) -> UInt256 {
        self.hash
    }
}

impl InventoryHash<UInt256> for RelayExtensibleEntry {
    fn inventory_hash(&self) -> &UInt256 {
        &self.hash
    }
}

/// Type alias for the relay cache storing extensible payloads.
pub(crate) type RelayExtensibleCache = RelayCache<UInt256, RelayExtensibleEntry>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relay_cache_capacity_is_reasonable() {
        assert_eq!(RELAY_CACHE_CAPACITY, 100);
    }

    #[test]
    fn ledger_hydration_window_is_reasonable() {
        assert_eq!(LEDGER_HYDRATION_WINDOW, 2_000);
    }
}
