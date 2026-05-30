//! Network-specific connection configuration.

use crate::NetworkType;
use serde::{Deserialize, Serialize};

/// Network-specific configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NetworkConfig {
    /// Network type
    pub network_type: NetworkType,

    /// Custom network magic (overrides default if set)
    pub magic: Option<u32>,

    /// Custom address version (overrides default if set)
    pub address_version: Option<u8>,

    /// Seed nodes to connect to
    pub seed_nodes: Vec<String>,

    /// Maximum number of peer connections
    #[serde(default = "default_max_peers")]
    pub max_peers: usize,

    /// Minimum number of peer connections to maintain
    #[serde(default = "default_min_peers")]
    pub min_peers: usize,

    /// Connection timeout in milliseconds
    #[serde(default = "default_connection_timeout")]
    pub connection_timeout_ms: u64,
}

const fn default_max_peers() -> usize {
    50
}

const fn default_min_peers() -> usize {
    10
}

const fn default_connection_timeout() -> u64 {
    5000
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self::for_network(NetworkType::MainNet)
    }
}

impl NetworkConfig {
    /// Create configuration for a specific network type
    #[must_use]
    pub fn for_network(network_type: NetworkType) -> Self {
        Self {
            network_type,
            magic: None,
            address_version: None,
            seed_nodes: network_type.seed_nodes(),
            max_peers: default_max_peers(),
            min_peers: default_min_peers(),
            connection_timeout_ms: default_connection_timeout(),
        }
    }

    /// Get the effective network magic
    #[must_use]
    pub fn effective_magic(&self) -> u32 {
        self.magic.unwrap_or_else(|| self.network_type.magic())
    }

    /// Get the effective address version
    #[must_use]
    pub fn effective_address_version(&self) -> u8 {
        self.address_version
            .unwrap_or_else(|| self.network_type.address_version())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_effective_values() {
        let mut config = NetworkConfig::for_network(NetworkType::MainNet);
        assert_eq!(config.effective_magic(), 860833102);

        config.magic = Some(12345);
        assert_eq!(config.effective_magic(), 12345);
    }
}
