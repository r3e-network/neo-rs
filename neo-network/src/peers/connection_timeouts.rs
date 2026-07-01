//! Per-connection inactivity timeouts.
//!
//! Mirrors the two constants in C# `Connection.cs`:
//!
//! - `connectionTimeoutLimitStart` (10 s) — armed when the connection
//!   is created, before any data has been received. A peer that never
//!   completes (or even starts) the version handshake is dropped when
//!   it fires.
//! - `connectionTimeoutLimit` (60 s) — re-armed after every received
//!   payload.
//!
//! C# resets the timer on every raw TCP segment; the Rust read loop
//! resets it on every *decoded frame*. The Rust behaviour is the
//! stricter of the two (a peer trickling a single oversized frame
//! byte-by-byte cannot keep the connection alive indefinitely), which
//! only ever disconnects sooner than C# would.

use std::time::Duration;

/// Inactivity timeouts applied by the
/// [`crate::remote_node::RemoteNodeService`] read loop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ConnectionTimeouts {
    /// Time allowed before the first frame arrives
    /// (C# `connectionTimeoutLimitStart` = 10 s).
    pub initial: Duration,
    /// Idle time allowed between subsequent frames
    /// (C# `connectionTimeoutLimit` = 60 s).
    pub idle: Duration,
}

impl ConnectionTimeouts {
    /// C# `Connection.connectionTimeoutLimitStart`.
    pub const DEFAULT_INITIAL: Duration = Duration::from_secs(10);
    /// C# `Connection.connectionTimeoutLimit`.
    pub const DEFAULT_IDLE: Duration = Duration::from_secs(60);
}

impl Default for ConnectionTimeouts {
    fn default() -> Self {
        Self {
            initial: Self::DEFAULT_INITIAL,
            idle: Self::DEFAULT_IDLE,
        }
    }
}

#[cfg(test)]
#[path = "../tests/peers/connection_timeouts.rs"]
mod tests;
