//! Stable per-peer identifier.
//!
//! `PeerId` is the canonical way to refer to a single connected peer
//! across the network service. In the reth-style model each accepted
//! TCP connection spawns a [`crate::remote_node::RemoteNodeService`]
//! task; the `PeerId` is the handle the local node uses to look up
//! that task in its registry.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

/// Globally-unique 64-bit peer identifier.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, Ord, PartialEq, PartialOrd)]
pub struct PeerId(u64);

impl PeerId {
    /// Allocate a fresh, globally-unique peer identifier.
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }

    /// Construct a `PeerId` from a raw 64-bit value. Useful for
    /// tests and for restoring a peer id from a log / event stream.
    pub fn from_raw(raw: u64) -> Self {
        Self(raw)
    }

    /// Raw 64-bit value of this peer id.
    pub fn raw(self) -> u64 {
        self.0
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "peer:{}", self.0)
    }
}
