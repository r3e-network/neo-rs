//! Mirror of `Neo.Network.P2P.ChannelsConfig`.
//!
//! The C# implementation exposes a simple configuration object used when
//! bootstrapping the `LocalNode`.  It primarily controls connection limits and
//! whether compression is enabled.  We provide an equivalent Rust structure so
//! higher layers can depend on the same semantics during the porting effort.

use std::net::SocketAddr;
use std::time::Duration;

use super::framed::FrameConfig;

/// Configuration used to bootstrap the local P2P node.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChannelsConfig {
    /// TCP listener endpoint (optional – matches C# nullable `IPEndPoint`).
    pub tcp: Option<SocketAddr>,
    /// Whether compression is enabled for the link.
    pub enable_compression: bool,
    /// Minimum number of peers we would like to stay connected to.
    pub min_desired_connections: usize,
    /// Hard cap on simultaneously connected peers.
    pub max_connections: usize,
    /// Maximum simultaneous connections allowed from the same address.
    pub max_connections_per_address: usize,
    /// Number of inventory hashes we keep track of to avoid duplicates.
    pub max_known_hashes: usize,
    /// Maximum number of recent broadcasts to retain for diagnostics (0 disables retention).
    pub broadcast_history_limit: usize,
    /// Handshake receive timeout.
    pub handshake_timeout: Duration,
    /// Active-session receive timeout.
    pub read_timeout_active: Duration,
    /// Write timeout applied to outbound messages.
    pub write_timeout: Duration,
    /// Maximum time to wait for a socket shutdown during teardown.
    pub shutdown_timeout: Duration,
}

impl ChannelsConfig {
    /// Default compression behaviour (mirrors `ChannelsConfig.DefaultEnableCompression`).
    pub const DEFAULT_ENABLE_COMPRESSION: bool = true;
    /// Default minimum desired connections (mirrors C# constant).
    pub const DEFAULT_MIN_DESIRED_CONNECTIONS: usize = 10;
    /// Default maximum number of concurrent peers (4 × minimum).
    pub const DEFAULT_MAX_CONNECTIONS: usize = Self::DEFAULT_MIN_DESIRED_CONNECTIONS * 4;
    /// Default per-address connection cap (mirrors `DefaultMaxConnectionsPerAddress`).
    pub const DEFAULT_MAX_CONNECTIONS_PER_ADDRESS: usize = 3;
    /// Default size of the known-hash cache.
    pub const DEFAULT_MAX_KNOWN_HASHES: usize = 1000;
    /// Default number of broadcast history entries to retain.
    pub const DEFAULT_BROADCAST_HISTORY_LIMIT: usize = 1024;
    /// Default handshake receive timeout.
    pub const DEFAULT_HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(15);
    /// Default active read timeout.
    pub const DEFAULT_READ_TIMEOUT_ACTIVE: Duration = Duration::from_secs(120);
    /// Default write timeout.
    pub const DEFAULT_WRITE_TIMEOUT: Duration = Duration::from_secs(30);
    /// Default shutdown timeout.
    pub const DEFAULT_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(5);

    /// Creates a new configuration with optional overrides.
    pub fn new(tcp: Option<SocketAddr>) -> Self {
        Self {
            tcp,
            ..Self::default()
        }
    }

    /// Build a framed I/O configuration from the channel settings.
    pub fn frame_config(&self) -> FrameConfig {
        FrameConfig::from(self)
    }

    /// Convenience helper to build a `PeerConnection` using this configuration.
    pub fn build_connection(
        &self,
        stream: tokio::net::TcpStream,
        address: SocketAddr,
        inbound: bool,
    ) -> crate::network::p2p::connection::PeerConnection {
        crate::network::p2p::connection::PeerConnection::from_channels_config(
            stream, address, inbound, self,
        )
    }
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn build_connection_applies_frame_timeouts() {
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let addr = listener.local_addr().expect("addr");

        let client = tokio::net::TcpStream::connect(addr);
        let server = listener.accept();
        let (client_stream, server_stream) = tokio::join!(client, server);
        let client_stream = client_stream.expect("client stream");
        let (server_stream, _remote) = server_stream.expect("server stream");
        let remote_addr = server_stream.peer_addr().expect("peer addr");

        let config = ChannelsConfig {
            handshake_timeout: Duration::from_millis(7),
            read_timeout_active: Duration::from_millis(11),
            write_timeout: Duration::from_millis(13),
            shutdown_timeout: Duration::from_millis(17),
            ..ChannelsConfig::default()
        };

        let expected_frame = config.frame_config();
        let connection = config.build_connection(client_stream, remote_addr, true);

        assert_eq!(connection.frame_config, expected_frame);

        drop(connection);
        drop(server_stream);
    }
}

impl Default for ChannelsConfig {
    fn default() -> Self {
        Self {
            tcp: None,
            enable_compression: Self::DEFAULT_ENABLE_COMPRESSION,
            min_desired_connections: Self::DEFAULT_MIN_DESIRED_CONNECTIONS,
            max_connections: Self::DEFAULT_MAX_CONNECTIONS,
            max_connections_per_address: Self::DEFAULT_MAX_CONNECTIONS_PER_ADDRESS,
            max_known_hashes: Self::DEFAULT_MAX_KNOWN_HASHES,
            broadcast_history_limit: Self::DEFAULT_BROADCAST_HISTORY_LIMIT,
            handshake_timeout: Self::DEFAULT_HANDSHAKE_TIMEOUT,
            read_timeout_active: Self::DEFAULT_READ_TIMEOUT_ACTIVE,
            write_timeout: Self::DEFAULT_WRITE_TIMEOUT,
            shutdown_timeout: Self::DEFAULT_SHUTDOWN_TIMEOUT,
        }
    }
}
