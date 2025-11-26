//! Re-export TriggerType from the VM crate so trigger semantics stay aligned.

use bitflags::bitflags;
use std::str::FromStr;

bitflags! {
    /// Represents the triggers for running smart contracts (matches C# TriggerType)
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    pub const OnPersist: TriggerType = TriggerType::ON_PERSIST;
    pub const PostPersist: TriggerType = TriggerType::POST_PERSIST;
    pub const Verification: TriggerType = TriggerType::VERIFICATION;
    pub const Application: TriggerType = TriggerType::APPLICATION;
    pub const System: TriggerType = TriggerType::SYSTEM;
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
