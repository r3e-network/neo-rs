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

use neo_core::persistence::StoreCache;
use neo_core::smart_contract::{ApplicationEngine, LogEventArgs, NotifyEventArgs};
use neo_primitives::UInt256;
use neo_protocol::protocol_settings::ProtocolSettings;
use std::sync::Arc;

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
