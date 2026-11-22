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
    #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
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
        /// Huyao hardfork (introduced in Neo 3.10).
        HfHuyao = 7 => "HF_Huyao",
        /// Iara hardfork. First hardfork activated on-chain via `Policy.activateHardfork`.
        HfIara = 8 => "HF_Iara",
        /// Jackalope hardfork.
        HfJackalope = 9 => "HF_Jackalope",
    }
}

impl Hardfork {
    /// The last hardfork that is activated exclusively through
    /// `ProtocolSettings` configuration. Later hardforks are activated on-chain
    /// through Policy's `activateHardfork` call.
    ///
    /// Matches C# `ProtocolSettings.LastConfigManagedHardfork`.
    pub const LAST_CONFIG_MANAGED: Self = Self::HfHuyao;

    /// Returns whether this hardfork is managed by `ProtocolSettings` rather
    /// than by an on-chain Policy activation record.
    #[must_use]
    pub const fn is_config_managed(&self) -> bool {
        (*self as u8) <= (Self::LAST_CONFIG_MANAGED as u8)
    }

    /// Returns the hardfork name as a string.
    #[must_use]
    pub const fn name(&self) -> &'static str {
        self.as_str()
    }

    /// Returns the on-chain name of this hardfork: the enum identifier with
    /// the `HF_` prefix removed (e.g. `Iara`).
    ///
    /// Matches C# `Hardforks.GetName`; this is the exact string used as the
    /// Policy storage key for committee-activated hardforks.
    #[must_use]
    pub const fn raw_name(&self) -> &'static str {
        match self {
            Self::HfAspidochelone => "Aspidochelone",
            Self::HfBasilisk => "Basilisk",
            Self::HfCockatrice => "Cockatrice",
            Self::HfDomovoi => "Domovoi",
            Self::HfEchidna => "Echidna",
            Self::HfFaun => "Faun",
            Self::HfGorgon => "Gorgon",
            Self::HfHuyao => "Huyao",
            Self::HfIara => "Iara",
            Self::HfJackalope => "Jackalope",
        }
    }

    /// Parses a Policy hardfork name with the strict rules of C#
    /// `Hardforks.TryParseExact`.
    ///
    /// Only the full raw name is accepted, case-sensitively: `Iara` parses,
    /// while `iara`, `HF_Iara` and `8` do not. Returns `None` for anything else.
    #[must_use]
    pub fn try_parse_exact(name: &str) -> Option<Self> {
        let hardfork = Self::all()
            .iter()
            .copied()
            .find(|hardfork| hardfork.raw_name() == name)?;
        Some(hardfork)
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
            "HF_HUYAO" | "HUYAO" => Ok(Self::HfHuyao),
            "HF_IARA" | "IARA" => Ok(Self::HfIara),
            "HF_JACKALOPE" | "JACKALOPE" => Ok(Self::HfJackalope),
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
mod tests {
    use super::*;

    #[test]
    fn test_hardfork_all() {
        let all = Hardfork::all();
        assert_eq!(all.len(), 10);
        assert_eq!(Hardfork::COUNT, 10);
        assert_eq!(Hardfork::ALL, all);
        assert_eq!(all[0], Hardfork::HfAspidochelone);
        assert_eq!(all[6], Hardfork::HfGorgon);
        assert_eq!(all[7], Hardfork::HfHuyao);
        assert_eq!(all[8], Hardfork::HfIara);
        assert_eq!(all[9], Hardfork::HfJackalope);
    }

    #[test]
    fn test_last_config_managed_hardfork_matches_csharp() {
        // C# `ProtocolSettings.LastConfigManagedHardfork`.
        assert_eq!(Hardfork::LAST_CONFIG_MANAGED, Hardfork::HfHuyao);
        assert!(Hardfork::HfHuyao.is_config_managed());
        assert!(!Hardfork::HfIara.is_config_managed());
        assert!(!Hardfork::HfJackalope.is_config_managed());
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
        assert_eq!(Hardfork::from_index(7), Some(Hardfork::HfHuyao));
        assert_eq!(Hardfork::from_index(8), Some(Hardfork::HfIara));
        assert_eq!(Hardfork::from_index(9), Some(Hardfork::HfJackalope));
        assert_eq!(Hardfork::from_index(10), None);
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
    fn test_hardfork_raw_name_strips_prefix() {
        assert_eq!(Hardfork::HfAspidochelone.raw_name(), "Aspidochelone");
        assert_eq!(Hardfork::HfHuyao.raw_name(), "Huyao");
        assert_eq!(Hardfork::HfIara.raw_name(), "Iara");
        assert_eq!(Hardfork::HfJackalope.raw_name(), "Jackalope");
    }

    #[test]
    fn test_hardfork_try_parse_exact_matches_csharp() {
        assert_eq!(Hardfork::try_parse_exact("Iara"), Some(Hardfork::HfIara));
        assert_eq!(
            Hardfork::try_parse_exact("Jackalope"),
            Some(Hardfork::HfJackalope)
        );
        assert_eq!(Hardfork::try_parse_exact("Huyao"), Some(Hardfork::HfHuyao));

        // C# `Hardforks.TryParseExact` is case-sensitive and rejects the
        // `HF_` prefixed form as well as numeric names.
        assert_eq!(Hardfork::try_parse_exact("iara"), None);
        assert_eq!(Hardfork::try_parse_exact("IARA"), None);
        assert_eq!(Hardfork::try_parse_exact("HF_Iara"), None);
        assert_eq!(Hardfork::try_parse_exact("8"), None);
        assert_eq!(Hardfork::try_parse_exact(""), None);
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
        assert_eq!(Hardfork::try_from(7u8).unwrap(), Hardfork::HfHuyao);
        assert_eq!(Hardfork::try_from(8u8).unwrap(), Hardfork::HfIara);
        assert!(Hardfork::try_from(10u8).is_err());
    }

    #[test]
    fn test_hardfork_serde() {
        let hf = Hardfork::HfEchidna;
        let json = serde_json::to_string(&hf).unwrap();
        assert_eq!(json, "\"HfEchidna\"");
        let parsed: Hardfork = serde_json::from_str(&json).unwrap();
        assert_eq!(hf, parsed);
    }
}
