//! Hardfork enumeration for Neo blockchain.
//!
//! Matches C# Neo.Hardfork enum exactly.

use crate::protocol_enum_repr;
use serde::{Deserialize, Serialize};
use std::str::FromStr;

protocol_enum_repr! {
    all;

    /// Represents a hardfork in the Neo blockchain (matches C# Hardfork enum exactly).
    ///
    /// Hardforks are named after mythological creatures in alphabetical order.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
    pub Hardfork {
        /// Aspidochelone hardfork - First Neo N3 hardfork.
        HfAspidochelone = 0 => "HF_Aspidochelone",
        /// Basilisk hardfork.
        HfBasilisk = 1 => "HF_Basilisk",
        /// Cockatrice hardfork.
        HfCockatrice = 2 => "HF_Cockatrice",
        /// Domovoi hardfork.
        HfDomovoi = 3 => "HF_Domovoi",
        /// Echidna hardfork.
        HfEchidna = 4 => "HF_Echidna",
        /// Faun hardfork.
        HfFaun = 5 => "HF_Faun",
        /// Gorgon hardfork.
        HfGorgon = 6 => "HF_Gorgon",
    }
}

impl Hardfork {
    /// Returns the hardfork name as a string.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.as_str()
    }

    /// Returns the hardfork index (0-based).
    #[must_use]
    pub const fn index(&self) -> u8 {
        self.to_byte()
    }

    /// Creates a hardfork from its index.
    #[must_use]
    pub const fn from_index(index: u8) -> Option<Self> {
        Self::from_byte(index)
    }
}

impl FromStr for Hardfork {
    type Err = HardforkParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().to_ascii_uppercase();
        match normalized.as_str() {
            "HF_ASPIDOCHELONE" | "ASPIDOCHELONE" | "ASP" => Ok(Self::HfAspidochelone),
            "HF_BASILISK" | "BASILISK" => Ok(Self::HfBasilisk),
            "HF_COCKATRICE" | "COCKATRICE" => Ok(Self::HfCockatrice),
            "HF_DOMOVOI" | "DOMOVOI" => Ok(Self::HfDomovoi),
            "HF_ECHIDNA" | "ECHIDNA" => Ok(Self::HfEchidna),
            "HF_FAUN" | "FAUN" => Ok(Self::HfFaun),
            "HF_GORGON" | "GORGON" => Ok(Self::HfGorgon),
            _ => Err(HardforkParseError(value.to_string())),
        }
    }
}

impl TryFrom<u8> for Hardfork {
    type Error = HardforkParseError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::from_byte(value).ok_or_else(|| HardforkParseError(value.to_string()))
    }
}

/// Error returned when parsing a hardfork from a string fails.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("Unknown hardfork: '{0}'")]
pub struct HardforkParseError(pub String);

#[cfg(test)]
#[path = "tests/hardfork.rs"]
mod tests;
