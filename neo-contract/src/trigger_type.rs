//! TriggerType - matches C# Neo.SmartContract.TriggerType exactly

use bitflags::bitflags;
use std::str::FromStr;

bitflags! {
    /// Represents the triggers for running smart contracts (matches C# TriggerType)
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct TriggerType: u8 {
        /// Indicate that the contract is triggered by the system to execute the OnPersist method of the native contracts.
        const ON_PERSIST = 0x01;
        /// Indicate that the contract is triggered by the system to execute the PostPersist method of the native contracts.
        const POST_PERSIST = 0x02;
        /// Indicates that the contract is triggered by the verification of an IVerifiable.
        const VERIFICATION = 0x20;
        /// Indicates that the contract is triggered by the execution of transactions.
        const APPLICATION = 0x40;
        /// The combination of all system triggers.
        const SYSTEM = Self::ON_PERSIST.bits() | Self::POST_PERSIST.bits();
        /// The combination of all triggers.
        const ALL = Self::ON_PERSIST.bits()
            | Self::POST_PERSIST.bits()
            | Self::VERIFICATION.bits()
            | Self::APPLICATION.bits();
    }
}

#[allow(non_upper_case_globals)]
impl TriggerType {
    /// Alias for ON_PERSIST (C# naming convention)
    pub const OnPersist: TriggerType = TriggerType::ON_PERSIST;
    /// Alias for POST_PERSIST (C# naming convention)
    pub const PostPersist: TriggerType = TriggerType::POST_PERSIST;
    /// Alias for VERIFICATION (C# naming convention)
    pub const Verification: TriggerType = TriggerType::VERIFICATION;
    /// Alias for APPLICATION (C# naming convention)
    pub const Application: TriggerType = TriggerType::APPLICATION;
    /// Alias for SYSTEM (C# naming convention)
    pub const System: TriggerType = TriggerType::SYSTEM;
    /// Alias for ALL (C# naming convention)
    pub const All: TriggerType = TriggerType::ALL;
}

impl FromStr for TriggerType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            s if s.eq_ignore_ascii_case("OnPersist") => Ok(TriggerType::ON_PERSIST),
            s if s.eq_ignore_ascii_case("PostPersist") => Ok(TriggerType::POST_PERSIST),
            s if s.eq_ignore_ascii_case("Verification") => Ok(TriggerType::VERIFICATION),
            s if s.eq_ignore_ascii_case("Application") => Ok(TriggerType::APPLICATION),
            s if s.eq_ignore_ascii_case("System") => Ok(TriggerType::SYSTEM),
            s if s.eq_ignore_ascii_case("All") => Ok(TriggerType::ALL),
            _ => Err(format!("unknown trigger type: {s}")),
        }
    }
}

impl std::fmt::Display for TriggerType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if *self == TriggerType::ON_PERSIST {
            write!(f, "OnPersist")
        } else if *self == TriggerType::POST_PERSIST {
            write!(f, "PostPersist")
        } else if *self == TriggerType::VERIFICATION {
            write!(f, "Verification")
        } else if *self == TriggerType::APPLICATION {
            write!(f, "Application")
        } else if *self == TriggerType::SYSTEM {
            write!(f, "System")
        } else if *self == TriggerType::ALL {
            write!(f, "All")
        } else {
            write!(f, "TriggerType(0x{:02x})", self.bits())
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_trigger_type_values() {
        assert_eq!(TriggerType::ON_PERSIST.bits(), 0x01);
        assert_eq!(TriggerType::POST_PERSIST.bits(), 0x02);
        assert_eq!(TriggerType::VERIFICATION.bits(), 0x20);
        assert_eq!(TriggerType::APPLICATION.bits(), 0x40);
    }

    #[test]
    fn test_trigger_type_from_str() {
        assert_eq!(
            TriggerType::from_str("OnPersist").unwrap(),
            TriggerType::ON_PERSIST
        );
        assert_eq!(
            TriggerType::from_str("Application").unwrap(),
            TriggerType::APPLICATION
        );
        assert!(TriggerType::from_str("Invalid").is_err());
    }

    #[test]
    fn test_trigger_type_display() {
        assert_eq!(TriggerType::APPLICATION.to_string(), "Application");
        assert_eq!(TriggerType::VERIFICATION.to_string(), "Verification");
    }

    #[test]
    fn test_system_trigger() {
        let system = TriggerType::SYSTEM;
        assert!(system.contains(TriggerType::ON_PERSIST));
        assert!(system.contains(TriggerType::POST_PERSIST));
        assert!(!system.contains(TriggerType::APPLICATION));
    }
}
