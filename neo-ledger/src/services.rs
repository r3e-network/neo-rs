//! Service trait definitions for the ledger subsystem.
//!
//! These traits were originally in `neo-core::services::traits` and have been
//! distributed to their canonical crate locations.

/// Ledger-facing operations exposed to other subsystems.
///
/// Provides read-only access to blockchain state including block heights
/// and hash lookups.
pub trait LedgerService: Send + Sync {
    /// Current persisted block height.
    fn current_height(&self) -> u32;

    /// Highest header height observed in memory.
    fn current_header_height(&self) -> u32;

    /// Block hash at the given index when known.
    fn block_hash_at(&self, index: u32) -> Option<[u8; 32]>;
}

#[cfg(test)]
mod tests {
    use super::*;

    struct MockLedger {
        height: u32,
        header_height: u32,
    }

    impl LedgerService for MockLedger {
        fn current_height(&self) -> u32 {
            self.height
        }

        fn current_header_height(&self) -> u32 {
            self.header_height
        }

        fn block_hash_at(&self, _index: u32) -> Option<[u8; 32]> {
            None
        }
    }

    #[test]
    fn mock_ledger_service_works() {
        let ledger = MockLedger {
            height: 100,
            header_height: 105,
        };

        assert_eq!(ledger.current_height(), 100);
        assert_eq!(ledger.current_header_height(), 105);
        assert!(ledger.block_hash_at(0).is_none());
    }

    #[test]
    fn trait_is_object_safe() {
        fn _accepts_ledger(_: &dyn LedgerService) {}
    }
}
