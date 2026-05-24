use std::fmt;
use std::net::SocketAddr;

// ============ Peer Types ============

/// Unique identifier for a peer connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct PeerId(pub u64);

impl PeerId {
    /// Create a new peer ID.
    #[must_use]
    pub const fn new(id: u64) -> Self {
        Self(id)
    }

    /// Get the inner ID value.
    #[must_use]
    pub const fn inner(&self) -> u64 {
        self.0
    }
}

impl std::fmt::Display for PeerId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Peer({})", self.0)
    }
}

/// Information about a connected peer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PeerInfo {
    /// Unique peer identifier.
    pub id: PeerId,
    /// Remote address (IP:port).
    pub address: String,
    /// Protocol version.
    pub version: u32,
    /// Unix timestamp when connected.
    pub connected_at: u64,
    /// Start height reported by peer.
    pub start_height: u32,
    /// User agent string.
    pub user_agent: String,
}

impl PeerInfo {
    /// Create new peer info.
    #[must_use]
    pub const fn new(
        id: PeerId,
        address: String,
        version: u32,
        connected_at: u64,
        start_height: u32,
        user_agent: String,
    ) -> Self {
        Self {
            id,
            address,
            version,
            connected_at,
            start_height,
            user_agent,
        }
    }
}
