//! Service trait definitions for Neo subsystems.
//!
//! These traits were originally in the `neo-services` crate but have been
//! inlined here to reduce workspace complexity.

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

use crate::persistence::StoreCache;
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::{ApplicationEngine, LogEventArgs, NotifyEventArgs};
use neo_primitives::UInt256;
use std::sync::Arc;

/// System context trait providing access to core runtime services.
///
/// This trait abstracts the runtime context, allowing protocol-level code
/// to access necessary services without depending on the concrete runtime
/// implementation. The concrete implementation lives in neo-node.
///
/// # Design Rationale
///
/// By defining this trait in neo-core, we achieve:
/// - **Decoupling**: Protocol code doesn't depend on actor framework
/// - **Testability**: Easy to mock for unit tests
/// - **Flexibility**: Runtime can be swapped without changing protocol code
pub trait SystemContext: Send + Sync {
    /// Returns a snapshot of the current store cache for read operations.
    fn store_cache(&self) -> StoreCache;

    /// Returns the protocol settings for the current network.
    fn protocol_settings(&self) -> Arc<ProtocolSettings>;

    /// Returns the current block index (height).
    fn current_block_index(&self) -> u32;

    /// Returns the block hash at the specified index if known.
    fn block_hash_at(&self, index: u32) -> Option<UInt256>;

    /// Returns the current memory pool transaction count.
    fn mempool_count(&self) -> usize;

    /// Checks if a transaction exists in the mempool.
    fn mempool_contains(&self, hash: &UInt256) -> bool;

    /// Returns the highest header index observed.
    fn header_height(&self) -> u32;

    /// Returns true if the system is ready (synced within acceptable lag).
    fn is_ready(&self) -> bool;

    /// Dispatches an ApplicationEngine log event to registered handlers.
    fn notify_application_log(&self, engine: &ApplicationEngine, args: &LogEventArgs);

    /// Dispatches an ApplicationEngine notification event to registered handlers.
    fn notify_application_notify(&self, engine: &ApplicationEngine, args: &NotifyEventArgs);
}

#[cfg(test)]
mod tests {
    use super::*;

    // Mock implementations for testing
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
