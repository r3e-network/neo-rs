//! WebSocket subscription support for real-time events
//!
//! Provides WebSocket endpoint for subscribing to blockchain events:
//! - `block_added` - New block committed
//! - `transaction_added` - Transaction added to mempool
//! - `transaction_removed` - Transaction removed from mempool
//! - `notification` - Contract notification events
//!
//! The `handler.rs` module (legacy `warp::ws`-based per-connection loop)
//! was removed in the `2026-06-13-comprehensive-refactoring` change
//! (Phase C1). WebSocket subscriptions are now served by `jsonrpsee`'s
//! built-in WS transport; this module retains the event-broadcast
//! primitives and `SubscriptionManager` that the new transport still
//! consumes.

mod bridge;
mod events;
mod subscription;

pub use bridge::{SharedWsEventBridge, WsEventBridge};
pub use events::{WsEvent, WsEventType, WsNotification};
pub use subscription::{SubscriptionId, SubscriptionManager};
