//! Neo network type (MainNet / TestNet / Private).

use neo_primitives::constants;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Unsupported network selector supplied by an operator-facing config.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
#[error("unsupported Neo network type {value:?}")]
pub struct NetworkTypeParseError {
    value: String,
}

/// Neo network type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NetworkType {
    /// Neo `MainNet` (network magic: 860833102)
    MainNet,
    /// Neo `TestNet` T5 (network magic: 894710606)
    TestNet,
    /// Private/local network
    Private,
}

impl NetworkType {
    /// Returns the canonical network magic for a built-in public network.
    /// Private networks must supply their identity through a complete spec.
    #[must_use]
    pub const fn canonical_magic(self) -> Option<u32> {
        match self {
            Self::MainNet => Some(constants::MAINNET_MAGIC),
            Self::TestNet => Some(constants::TESTNET_MAGIC),
            Self::Private => None,
        }
    }

    /// Returns the canonical address version for a built-in public network.
    #[must_use]
    pub const fn canonical_address_version(self) -> Option<u8> {
        match self {
            Self::MainNet | Self::TestNet => Some(constants::ADDRESS_VERSION),
            Self::Private => None,
        }
    }
}

impl std::str::FromStr for NetworkType {
    type Err = NetworkTypeParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "mainnet" | "main" => Ok(Self::MainNet),
            "testnet" | "test" => Ok(Self::TestNet),
            "private" | "local" => Ok(Self::Private),
            _ => Err(NetworkTypeParseError {
                value: s.to_string(),
            }),
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
#[path = "../tests/network/network_type.rs"]
mod tests;
