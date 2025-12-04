//! Hardfork enumeration for Neo blockchain.
//!
//! Matches C# Neo.Hardfork enum exactly.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Represents a hardfork in the Neo blockchain (matches C# Hardfork enum exactly).
///
/// Hardforks are named after mythological creatures in alphabetical order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[repr(u8)]
pub enum Hardfork {
    /// Aspidochelone hardfork - First Neo N3 hardfork.
    HfAspidochelone = 0,
    /// Basilisk hardfork.
    HfBasilisk = 1,
    /// Cockatrice hardfork.
    HfCockatrice = 2,
    /// Domovoi hardfork.
    HfDomovoi = 3,
    /// Echidna hardfork.
    HfEchidna = 4,
    /// Faun hardfork.
    HfFaun = 5,
    /// Gorgon hardfork.
    HfGorgon = 6,
}

impl Hardfork {
    /// Returns all known hardforks in declaration order.
    pub const fn all() -> [Hardfork; 7] {
        [
            Hardfork::HfAspidochelone,
            Hardfork::HfBasilisk,
            Hardfork::HfCockatrice,
            Hardfork::HfDomovoi,
            Hardfork::HfEchidna,
            Hardfork::HfFaun,
            Hardfork::HfGorgon,
        ]
    }

    /// Returns the number of known hardforks.
    pub const fn count() -> usize {
        7
    }

    /// Returns the hardfork name as a string.
    pub const fn name(&self) -> &'static str {
        match self {
            Hardfork::HfAspidochelone => "HF_Aspidochelone",
            Hardfork::HfBasilisk => "HF_Basilisk",
            Hardfork::HfCockatrice => "HF_Cockatrice",
            Hardfork::HfDomovoi => "HF_Domovoi",
            Hardfork::HfEchidna => "HF_Echidna",
            Hardfork::HfFaun => "HF_Faun",
            Hardfork::HfGorgon => "HF_Gorgon",
        }
    }

    /// Returns the hardfork index (0-based).
    pub const fn index(&self) -> u8 {
        *self as u8
    }

    /// Creates a hardfork from its index.
    pub const fn from_index(index: u8) -> Option<Self> {
        match index {
            0 => Some(Hardfork::HfAspidochelone),
            1 => Some(Hardfork::HfBasilisk),
            2 => Some(Hardfork::HfCockatrice),
            3 => Some(Hardfork::HfDomovoi),
            4 => Some(Hardfork::HfEchidna),
            5 => Some(Hardfork::HfFaun),
            6 => Some(Hardfork::HfGorgon),
            _ => None,
        }
    }
}

impl fmt::Display for Hardfork {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.name())
    }
}

impl FromStr for Hardfork {
    type Err = HardforkParseError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let normalized = value.trim().to_ascii_uppercase();
        match normalized.as_str() {
            "HF_ASPIDOCHELONE" | "ASPIDOCHELONE" | "ASP" => Ok(Hardfork::HfAspidochelone),
            "HF_BASILISK" | "BASILISK" => Ok(Hardfork::HfBasilisk),
            "HF_COCKATRICE" | "COCKATRICE" => Ok(Hardfork::HfCockatrice),
            "HF_DOMOVOI" | "DOMOVOI" => Ok(Hardfork::HfDomovoi),
            "HF_ECHIDNA" | "ECHIDNA" => Ok(Hardfork::HfEchidna),
            "HF_FAUN" | "FAUN" => Ok(Hardfork::HfFaun),
            "HF_GORGON" | "GORGON" => Ok(Hardfork::HfGorgon),
            _ => Err(HardforkParseError(value.to_string())),
        }
    }
}

impl TryFrom<u8> for Hardfork {
    type Error = HardforkParseError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Hardfork::from_index(value).ok_or_else(|| HardforkParseError(value.to_string()))
    }
}

/// Error returned when parsing a hardfork from a string fails.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HardforkParseError(pub String);

impl fmt::Display for HardforkParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Unknown hardfork: '{}'", self.0)
    }
}

impl std::error::Error for HardforkParseError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hardfork_all() {
        let all = Hardfork::all();
        assert_eq!(all.len(), 7);
        assert_eq!(all[0], Hardfork::HfAspidochelone);
        assert_eq!(all[6], Hardfork::HfGorgon);
    }

    #[test]
    fn test_hardfork_index() {
        assert_eq!(Hardfork::HfAspidochelone.index(), 0);
        assert_eq!(Hardfork::HfBasilisk.index(), 1);
        assert_eq!(Hardfork::HfCockatrice.index(), 2);
        assert_eq!(Hardfork::HfDomovoi.index(), 3);
        assert_eq!(Hardfork::HfEchidna.index(), 4);
        assert_eq!(Hardfork::HfFaun.index(), 5);
        assert_eq!(Hardfork::HfGorgon.index(), 6);
    }

    #[test]
    fn test_hardfork_from_index() {
        assert_eq!(Hardfork::from_index(0), Some(Hardfork::HfAspidochelone));
        assert_eq!(Hardfork::from_index(6), Some(Hardfork::HfGorgon));
        assert_eq!(Hardfork::from_index(7), None);
        assert_eq!(Hardfork::from_index(255), None);
    }

    #[test]
    fn test_hardfork_from_str() {
        assert_eq!(
            "HF_ASPIDOCHELONE".parse::<Hardfork>().unwrap(),
            Hardfork::HfAspidochelone
        );
        assert_eq!(
            "aspidochelone".parse::<Hardfork>().unwrap(),
            Hardfork::HfAspidochelone
        );
        assert_eq!(
            "ASP".parse::<Hardfork>().unwrap(),
            Hardfork::HfAspidochelone
        );
        assert_eq!(
            "HF_BASILISK".parse::<Hardfork>().unwrap(),
            Hardfork::HfBasilisk
        );
        assert_eq!(
            "basilisk".parse::<Hardfork>().unwrap(),
            Hardfork::HfBasilisk
        );
    }

    #[test]
    fn test_hardfork_from_str_invalid() {
        assert!("unknown".parse::<Hardfork>().is_err());
        assert!("".parse::<Hardfork>().is_err());
    }

    #[test]
    fn test_hardfork_display() {
        assert_eq!(Hardfork::HfAspidochelone.to_string(), "HF_Aspidochelone");
        assert_eq!(Hardfork::HfBasilisk.to_string(), "HF_Basilisk");
        assert_eq!(Hardfork::HfGorgon.to_string(), "HF_Gorgon");
    }

    #[test]
    fn test_hardfork_name() {
        assert_eq!(Hardfork::HfAspidochelone.name(), "HF_Aspidochelone");
        assert_eq!(Hardfork::HfEchidna.name(), "HF_Echidna");
    }

    #[test]
    fn test_hardfork_ordering() {
        assert!(Hardfork::HfAspidochelone < Hardfork::HfBasilisk);
        assert!(Hardfork::HfBasilisk < Hardfork::HfCockatrice);
        assert!(Hardfork::HfFaun < Hardfork::HfGorgon);
    }

    #[test]
    fn test_hardfork_try_from_u8() {
        assert_eq!(Hardfork::try_from(0u8).unwrap(), Hardfork::HfAspidochelone);
        assert_eq!(Hardfork::try_from(6u8).unwrap(), Hardfork::HfGorgon);
        assert!(Hardfork::try_from(7u8).is_err());
    }

    #[test]
    fn test_hardfork_serde() {
        let hf = Hardfork::HfEchidna;
        let json = serde_json::to_string(&hf).unwrap();
        let parsed: Hardfork = serde_json::from_str(&json).unwrap();
        assert_eq!(hf, parsed);
    }
}
