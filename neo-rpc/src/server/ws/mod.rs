//! WebSocket subscription support for real-time events
//!
//! Provides WebSocket endpoint for subscribing to blockchain events:
//! - `block_added` - New block committed
//! - `transaction_added` - Transaction added to mempool
//! - `transaction_removed` - Transaction removed from mempool
//! - `notification` - Contract notification events

mod bridge;
mod events;
mod handler;
mod subscription;

pub use bridge::{SharedWsEventBridge, WsEventBridge};
pub use events::{WsEvent, WsEventType, WsNotification};
pub use handler::ws_handler;
pub use subscription::{SubscriptionId, SubscriptionManager};
