//! WebSocket subscription support for real-time events
//!
//! Provides WebSocket endpoint for subscribing to blockchain events:
//! - `block_added` - New block committed
//! - `transaction_added` - Transaction added to mempool
//! - `transaction_removed` - Transaction removed from mempool
//! - `notification` - Contract notification events
//!
//! The legacy per-connection WebSocket loop was removed in the
//! `2026-06-13-comprehensive-refactoring` change (Phase C1). This module now
//! retains the event-broadcast primitives used by higher-level services;
//! per-client JSON-RPC subscription routing belongs in the active `jsonrpsee`
//! transport layer.

mod bridge;
mod events;

pub use bridge::{SharedWsEventBridge, WsEventBridge};
pub use events::{WsEvent, WsEventType, WsNotification};
