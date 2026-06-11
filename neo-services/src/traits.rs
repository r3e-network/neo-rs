//! Service trait definitions for Neo subsystems.
//!
//! These traits are the simple, foundation-level contracts that can be
//! implemented by any layer. The stateful `SystemContext` trait (which
//! touches the smart-contract engine, native contracts, actor handles,
//! etc.) remains in `neo-core` because it requires the full runtime
//! surface to be useful.
//!
//! ## Layering
//!
//! Sits in **Layer 1 (utility)**. Depends only on:
//! - `neo-primitives` (Layer 0) — for hash types.
//!
//! The simple traits here (`LedgerService`, `StateStoreService`,
//! `MempoolService`, `PeerManagerService`, `RpcService`) are pure
//! `Send + Sync` read-only contracts and can be implemented by mocks
//! in unit tests without pulling in the runtime.

#![allow(missing_docs)]

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

/// State store operations required by RPC/health checks.
///
/// Provides access to state root indices for monitoring sync progress.
pub trait StateStoreService: Send + Sync {
    /// Latest local state root index.
    fn local_root_index(&self) -> Option<u32>;

    /// Latest validated state root index.
    fn validated_root_index(&self) -> Option<u32>;
}

/// Mempool operations used across networking and RPC layers.
///
/// Provides basic statistics about the transaction pool.
pub trait MempoolService: Send + Sync {
    /// Total transaction count tracked by the pool.
    fn count(&self) -> usize;
}

/// Peer manager operations exposed to observability layers.
///
/// Provides peer connection statistics.
pub trait PeerManagerService: Send + Sync {
    /// Current connected peer count.
    fn peer_count(&self) -> usize;
}

/// Minimal RPC service readiness contract.
///
/// Used by health checks to determine if the RPC server is accepting requests.
pub trait RpcService: Send + Sync {
    /// Returns true when the service is ready to accept requests.
    fn is_started(&self) -> bool;
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

    struct MockMempool {
        count: usize,
    }

    impl MempoolService for MockMempool {
        fn count(&self) -> usize {
            self.count
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
    fn mock_mempool_service_works() {
        let mempool = MockMempool { count: 42 };
        assert_eq!(mempool.count(), 42);
    }

    #[test]
    fn traits_are_object_safe() {
        // Verify traits can be used as trait objects
        fn _accepts_ledger(_: &dyn LedgerService) {}
        fn _accepts_mempool(_: &dyn MempoolService) {}
        fn _accepts_state_store(_: &dyn StateStoreService) {}
        fn _accepts_peer_manager(_: &dyn PeerManagerService) {}
        fn _accepts_rpc(_: &dyn RpcService) {}
    }
}
