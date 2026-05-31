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

use crate::contains_transaction_type::ContainsTransactionType;
use crate::events::PluginEvent;
use crate::ledger::blockchain::BlockchainHandle;
use crate::ledger::{HeaderCache, LedgerContext, MemoryPool};
use crate::network::p2p::payloads::{
    block::Block, extensible_payload::ExtensiblePayload, header::Header, transaction::Transaction,
};
use crate::network::p2p::{LocalNodeHandle, TaskManagerHandle};
use crate::neo_system::actors::TransactionRouterHandle;
use crate::neo_system::NeoSystem;
use crate::persistence::StoreCache;
use crate::protocol_settings::ProtocolSettings;
use crate::runtime::{ActorSystemHandle, EventStreamHandle};
use crate::smart_contract::{ApplicationEngine, LogEventArgs, NotifyEventArgs};
use crate::state_service::StateStore;
use crate::CoreResult;
use neo_primitives::{UInt160, UInt256};
use parking_lot::Mutex;
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
pub trait SystemContext: Send + Sync + std::fmt::Debug {
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

    // --- Runtime/ledger handles used by the Blockchain and P2P actors ---
    // (Added for the A5 SystemContext inversion: actors program against the
    // trait rather than the concrete NeoSystemContext.)

    /// Effective protocol settings handle.
    fn settings(&self) -> Arc<ProtocolSettings>;

    /// Shared header cache.
    fn header_cache(&self) -> Arc<HeaderCache>;

    /// Ledger context (blockchain read/relay surface).
    fn ledger(&self) -> Arc<LedgerContext>;

    /// Shared memory pool.
    fn memory_pool(&self) -> Arc<Mutex<MemoryPool>>;

    /// Shared memory pool handle (alias used by some actors).
    fn memory_pool_handle(&self) -> Arc<Mutex<MemoryPool>>;

    /// Broadcast a plugin event to registered handlers.
    fn broadcast_plugin_event(&self, event: PluginEvent);

    /// Record an extensible payload for relay; returns its hash.
    fn record_extensible(&self, payload: ExtensiblePayload) -> CoreResult<UInt256>;

    /// Whether the node is in fast-sync mode.
    fn is_fast_sync_mode(&self) -> bool;

    /// The owning `NeoSystem`, if attached.
    fn neo_system(&self) -> Option<Arc<NeoSystem>>;

    /// The state store service, if available.
    fn state_store(&self) -> CoreResult<Option<Arc<StateStore>>>;

    /// Handle to the actor system (for event-stream publishing).
    fn actor_system(&self) -> &ActorSystemHandle;

    /// Handle to the local P2P node (for direct relay).
    fn local_node(&self) -> &LocalNodeHandle;

    // --- Handles + query methods used by the P2P actors (A5 part 2) ---

    /// Handle to the blockchain actor.
    fn blockchain(&self) -> &BlockchainHandle;

    /// Handle to the task-manager actor.
    fn task_manager(&self) -> &TaskManagerHandle;

    /// Handle to the transaction-router actor.
    fn tx_router(&self) -> &TransactionRouterHandle;

    /// Event stream handle for publishing/subscribing to system events.
    fn event_stream(&self) -> EventStreamHandle;

    /// Look up a block by hash (ledger).
    fn try_get_block(&self, hash: &UInt256) -> Option<Block>;

    /// Look up an extensible payload by hash.
    fn try_get_extensible(&self, hash: &UInt256) -> Option<ExtensiblePayload>;

    /// Look up an extensible payload queued for relay.
    fn try_get_relay_extensible(&self, hash: &UInt256) -> Option<ExtensiblePayload>;

    /// Block hashes starting from a given hash (inventory sync).
    fn block_hashes_from(&self, hash_start: &UInt256, count: usize) -> Vec<UInt256>;

    /// Look up a transaction in the mempool.
    fn try_get_transaction_from_mempool(&self, hash: &UInt256) -> Option<Transaction>;

    /// Whether a transaction is known (mempool/ledger).
    fn contains_transaction(&self, hash: &UInt256) -> ContainsTransactionType;

    /// Whether a conflict record exists for the given hash + signers.
    fn contains_conflict_hash(&self, hash: &UInt256, signers: &[UInt160]) -> bool;

    /// Headers starting from an index (header sync).
    fn headers_from_index(&self, index_start: u32, count: usize) -> Vec<Header>;

    /// All mempool transaction hashes.
    fn mempool_transaction_hashes(&self) -> Vec<UInt256>;
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
