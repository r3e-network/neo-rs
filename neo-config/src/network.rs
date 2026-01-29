//! Network type and configuration

use serde::{Deserialize, Serialize};

/// Neo network type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum NetworkType {
    /// Neo `MainNet` (network magic: 860833102)
    #[default]
    MainNet,
    /// Neo `TestNet` T5 (network magic: 894710606)
    TestNet,
    /// Private/local network
    Private,
}

impl NetworkType {
    /// Get the network magic number
    #[must_use]
    pub const fn magic(&self) -> u32 {
        match self {
            Self::MainNet => 860833102,  // 0x334F454E "NEO3" LE
            Self::TestNet => 894710606,  // T5 testnet
            Self::Private => 0x01020304, // Default private
        }
    }

    /// Get the address version byte
    #[must_use]
    pub const fn address_version(&self) -> u8 {
        match self {
            Self::MainNet => 0x35, // 'N'
            Self::TestNet => 0x35, // Same as mainnet
            Self::Private => 0x35, // Same as mainnet
        }
    }

    /// Get default seed nodes
    #[must_use]
    pub fn seed_nodes(&self) -> Vec<String> {
        match self {
            Self::MainNet => vec![
                "seed1.neo.org:10333".to_string(),
                "seed2.neo.org:10333".to_string(),
                "seed3.neo.org:10333".to_string(),
                "seed4.neo.org:10333".to_string(),
                "seed5.neo.org:10333".to_string(),
            ],
            Self::TestNet => vec![
                "seed1t5.neo.org:20333".to_string(),
                "seed2t5.neo.org:20333".to_string(),
                "seed3t5.neo.org:20333".to_string(),
                "seed4t5.neo.org:20333".to_string(),
                "seed5t5.neo.org:20333".to_string(),
            ],
            Self::Private => vec![],
        }
    }
}

impl std::str::FromStr for NetworkType {
    type Err = ();

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mainnet" | "main" => Ok(Self::MainNet),
            "testnet" | "test" => Ok(Self::TestNet),
            "private" | "local" => Ok(Self::Private),
            _ => Err(()),
        }
    }
}

impl std::fmt::Display for NetworkType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MainNet => write!(f, "mainnet"),
            Self::TestNet => write!(f, "testnet"),
            Self::Private => write!(f, "private"),
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
    fn test_network_magic() {
        assert_eq!(NetworkType::MainNet.magic(), 860833102);
        assert_eq!(NetworkType::TestNet.magic(), 894710606);
    }

    #[test]
    fn test_network_from_str() {
        assert_eq!(
            "mainnet".parse::<NetworkType>().ok(),
            Some(NetworkType::MainNet)
        );
        assert_eq!(
            "TESTNET".parse::<NetworkType>().ok(),
            Some(NetworkType::TestNet)
        );
        assert_eq!(
            "private".parse::<NetworkType>().ok(),
            Some(NetworkType::Private)
        );
        assert!("unknown".parse::<NetworkType>().is_err());
    }

    #[test]
    fn test_effective_values() {
        let mut config = NetworkConfig::for_network(NetworkType::MainNet);
        assert_eq!(config.effective_magic(), 860833102);

        config.magic = Some(12345);
        assert_eq!(config.effective_magic(), 12345);
    }
}
