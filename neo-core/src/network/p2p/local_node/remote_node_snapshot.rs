//! Immutable snapshot of a remote peer used for API exposure.

use super::*;

/// Immutable snapshot of a remote peer used for API exposure (matches C# RemoteNodeModel source data).
#[derive(Debug, Clone)]
pub struct RemoteNodeSnapshot {
    /// Remote socket endpoint as seen by the transport layer.
    pub remote_address: SocketAddr,
    /// Remote TCP port reported by the peer.
    pub remote_port: u16,
    /// Remote listener TCP port (advertised to the network).
    pub listen_tcp_port: u16,
    /// Last block height reported by the peer.
    pub last_block_index: u32,
    /// Protocol version of the peer.
    pub version: u32,
    /// Service bitmask advertised by the peer.
    pub services: u64,
    /// Unix timestamp (seconds) when the snapshot was captured.
    pub timestamp: u64,
}

impl RemoteNodeSnapshot {
    /// Updates the last block height and refreshes the timestamp.
    pub(super) fn touch(&mut self, last_block_index: u32, timestamp: u64) {
        self.last_block_index = last_block_index;
        self.timestamp = timestamp;
    }
}
