//! Neo network type (MainNet / TestNet / Private).

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

#[cfg(test)]
#[path = "tests/network_type.rs"]
mod tests;
