//! # neo-rpc::server::ws
//!
//! WebSocket events, bridges, and notification models.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `bridge`: WebSocket event bridge implementation.
//! - `events`: Mempool event records emitted to subscribers.

mod bridge;
mod events;

pub use bridge::{SharedWsEventBridge, WsEventBridge};
pub use events::{WsEvent, WsEventType, WsNotification};
