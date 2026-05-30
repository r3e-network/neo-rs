//! Information about a connected peer.

use super::peer::PeerId;

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
