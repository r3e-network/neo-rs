//! `WitnessConditionType` - matches C# Neo.Network.P2P.Payloads.WitnessConditionType exactly.
//!
//! This is the single source of truth for witness condition protocol bytes. Both
//! `neo-core::witness_rule` and `neo-p2p` re-export this type for backward compatibility.

use crate::protocol_enum;

protocol_enum! {
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    /// The type of witness condition.
    pub WitnessConditionType {
        /// Boolean condition.
        Boolean = 0x00,
        /// Not condition (logical NOT).
        Not = 0x01,
        /// And condition (logical AND).
        And = 0x02,
        /// Or condition (logical OR).
        Or = 0x03,
        /// Script hash condition.
        ScriptHash = 0x18,
        /// Group condition.
        Group = 0x19,
        /// Called by entry condition.
        CalledByEntry = 0x20,
        /// Called by contract condition.
        CalledByContract = 0x28,
        /// Called by group condition.
        CalledByGroup = 0x29,
    }
}

#[cfg(test)]
#[path = "../../tests/protocol/witness/witness_condition_type.rs"]
mod tests;
