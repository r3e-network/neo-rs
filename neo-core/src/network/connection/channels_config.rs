use std::net::SocketAddr;
use crate::network::Peer;

/// Represents the settings to start `LocalNode`.
#[derive(Clone, Debug)]
pub struct ChannelsConfig {
    /// Tcp configuration.
    pub tcp: SocketAddr,

    /// Minimum desired connections.
    pub min_desired_connections: u32,

    /// Max allowed connections.
    pub max_connections: u32,

    /// Max allowed connections per address.
    pub max_connections_per_address: u32,
}

impl Default for ChannelsConfig {
    fn default() -> Self {
        ChannelsConfig {
            tcp: SocketAddr::new(std::net::IpAddr::V4(std::net::Ipv4Addr::UNSPECIFIED), 0),
            min_desired_connections: Peer::DEFAULT_MIN_DESIRED_CONNECTIONS as u32,
            max_connections: Peer::DEFAULT_MAX_CONNECTIONS as u32,
            max_connections_per_address: 3,
        }
    }
}
