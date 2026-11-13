//! Service trait definitions for the system runtime subsystem.
//!
//! Distributed from `neo-core::services::traits`. The `SystemContext` trait
//! abstracts the runtime context so protocol-level code can access core
//! services without depending on the concrete runtime implementation.
//!
//! # Dependency note
//!
//! `neo-system` currently depends on `neo-core` (the canonical `NeoSystem`
//! runtime still lives there), so the context types referenced by this trait
//! (`StoreCache`, `ProtocolSettings`, `ApplicationEngine`, `LogEventArgs`,
//! `NotifyEventArgs`) are resolved through `neo-core`'s public surface.

use neo_primitives::UInt256;
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_smart_contract::ApplicationEngine;
use neo_storage::persistence::StoreCache;
use std::sync::Arc;

// Re-export mempool services
pub use crate::mempool::services::{LockedMempoolService, MempoolService};

// Re-export network services
pub use crate::network::p2p::services::PeerManagerService;

// Placeholder traits for services not yet fully implemented
pub trait LedgerService: Send + Sync {
    fn current_height(&self) -> u32;
    fn current_header_height(&self) -> u32;
}

pub trait StateStoreService: Send + Sync {
    fn on_new_state_root(&self, state_root: crate::state_service::StateRoot) -> bool;

    /// Stages local-root progress in the block's atomic precommit snapshot.
    fn stage_local_root(
        &self,
        state_root: &crate::state_service::StateRoot,
        snapshot: &neo_storage::persistence::DataCache,
    ) -> bool;

    /// Finishes a staged root after the block transaction commits successfully.
    fn on_local_root_committed(&self, state_root: &crate::state_service::StateRoot) -> bool;

    /// Discards a staged marker when the surrounding block transaction rolls back.
    fn discard_staged_local_root(&self, index: u32);

    /// Get the current local root index
    fn local_root_index(&self) -> u64;

    /// Get the current validated root index
    fn validated_root_index(&self) -> u64;

    /// Set a fetcher for reference state roots
    fn set_reference_root_fetcher(
        &self,
        fetcher: Arc<dyn Fn(u32) -> Option<UInt256> + Send + Sync>,
    );

    /// Flush pending state changes to storage
    fn flush(&self);
}

// Re-export event args types from their actual locations
pub use neo_primitives::LogEventArgs;
pub use neo_vm::NotifyEventArgs;

/// System context trait providing access to core runtime services.
///
/// This trait abstracts the runtime context, allowing protocol-level code
/// to access necessary services without depending on the concrete runtime
/// implementation. The concrete implementation lives in neo-node.
///
/// # Design Rationale
///
/// By defining this trait here, we achieve:
/// - **Decoupling**: Protocol code doesn't depend on the actor framework
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
