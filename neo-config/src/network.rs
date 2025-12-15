//! Network type and configuration

use serde::{Deserialize, Serialize};

/// Neo network type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NetworkType {
    /// Neo MainNet (network magic: 860833102)
    #[default]
    MainNet,
    /// Neo TestNet T5 (network magic: 894710606)
    TestNet,
    /// Private/local network
    Private,
}

impl NetworkType {
    /// Get the network magic number
    pub fn magic(&self) -> u32 {
        match self {
            NetworkType::MainNet => 860833102,  // 0x334F454E "NEO3" LE
            NetworkType::TestNet => 894710606,  // T5 testnet
            NetworkType::Private => 0x01020304, // Default private
        }
    }

    /// Get the address version byte
    pub fn address_version(&self) -> u8 {
        match self {
            NetworkType::MainNet => 0x35, // 'N'
            NetworkType::TestNet => 0x35, // Same as mainnet
            NetworkType::Private => 0x35, // Same as mainnet
        }
    }

    /// Get default seed nodes
    pub fn seed_nodes(&self) -> Vec<String> {
        match self {
            NetworkType::MainNet => vec![
                "seed1.neo.org:10333".to_string(),
                "seed2.neo.org:10333".to_string(),
                "seed3.neo.org:10333".to_string(),
                "seed4.neo.org:10333".to_string(),
                "seed5.neo.org:10333".to_string(),
            ],
            NetworkType::TestNet => vec![
                "seed1t5.neo.org:20333".to_string(),
                "seed2t5.neo.org:20333".to_string(),
                "seed3t5.neo.org:20333".to_string(),
                "seed4t5.neo.org:20333".to_string(),
                "seed5t5.neo.org:20333".to_string(),
            ],
            NetworkType::Private => vec![],
        }
    }

    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "mainnet" | "main" => Some(NetworkType::MainNet),
            "testnet" | "test" => Some(NetworkType::TestNet),
            "private" | "local" => Some(NetworkType::Private),
            _ => None,
        }
    }
}

impl std::fmt::Display for NetworkType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            NetworkType::MainNet => write!(f, "mainnet"),
            NetworkType::TestNet => write!(f, "testnet"),
            NetworkType::Private => write!(f, "private"),
        }
    }
}

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

fn default_max_peers() -> usize {
    50
}

fn default_min_peers() -> usize {
    10
}

fn default_connection_timeout() -> u64 {
    5000
}

impl Default for NetworkConfig {
    fn default() -> Self {
        Self::for_network(NetworkType::MainNet)
    }
}

impl NetworkConfig {
    /// Create configuration for a specific network type
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
    pub fn effective_magic(&self) -> u32 {
        self.magic.unwrap_or_else(|| self.network_type.magic())
    }

    /// Get the effective address version
    pub fn effective_address_version(&self) -> u8 {
        self.address_version.unwrap_or_else(|| self.network_type.address_version())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_network_magic() {
        assert_eq!(NetworkType::MainNet.magic(), 860833102);
        assert_eq!(NetworkType::TestNet.magic(), 894710606);
    }

    #[test]
    fn test_network_from_str() {
        assert_eq!(NetworkType::from_str("mainnet"), Some(NetworkType::MainNet));
        assert_eq!(NetworkType::from_str("TESTNET"), Some(NetworkType::TestNet));
        assert_eq!(NetworkType::from_str("private"), Some(NetworkType::Private));
        assert_eq!(NetworkType::from_str("unknown"), None);
    }

    #[test]
    fn test_effective_values() {
        let mut config = NetworkConfig::for_network(NetworkType::MainNet);
        assert_eq!(config.effective_magic(), 860833102);

        config.magic = Some(12345);
        assert_eq!(config.effective_magic(), 12345);
    }
}
