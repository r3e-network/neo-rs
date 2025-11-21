//! Mirror of `Neo.Network.P2P.ChannelsConfig`.
//!
//! The C# implementation exposes a simple configuration object used when
//! bootstrapping the `LocalNode`.  It primarily controls connection limits and
//! whether compression is enabled.  We provide an equivalent Rust structure so
//! higher layers can depend on the same semantics during the porting effort.

use std::net::SocketAddr;

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

    /// Creates a new configuration with optional overrides.
    pub fn new(tcp: Option<SocketAddr>) -> Self {
        Self {
            tcp,
            ..Self::default()
        }
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
        }
    }
}
