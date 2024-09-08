
use std::convert::TryFrom;

/// Represents the type of WitnessCondition.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WitnessConditionType {
    /// Indicates that the condition will always be met or not met.
    Boolean = 0x00,

    /// Reverse another condition.
    Not = 0x01,

    /// Indicates that all conditions must be met.
    And = 0x02,

    /// Indicates that any of the conditions meets.
    Or = 0x03,

    /// Indicates that the condition is met when the current context has the specified script hash.
    ScriptHash = 0x18,

    /// Indicates that the condition is met when the current context has the specified group.
    Group = 0x19,

    /// Indicates that the condition is met when the current context is the entry point or is called by the entry point.
    CalledByEntry = 0x20,

    /// Indicates that the condition is met when the current context is called by the specified contract.
    CalledByContract = 0x28,

    /// Indicates that the condition is met when the current context is called by the specified group.
    CalledByGroup = 0x29,
}

impl TryFrom<u8> for WitnessConditionType {
    type Error = ();

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0x00 => Ok(WitnessConditionType::Boolean),
            0x01 => Ok(WitnessConditionType::Not),
            0x02 => Ok(WitnessConditionType::And),
            0x03 => Ok(WitnessConditionType::Or),
            0x18 => Ok(WitnessConditionType::ScriptHash),
            0x19 => Ok(WitnessConditionType::Group),
            0x20 => Ok(WitnessConditionType::CalledByEntry),
            0x28 => Ok(WitnessConditionType::CalledByContract),
            0x29 => Ok(WitnessConditionType::CalledByGroup),
            _ => Err(()),
        }
    }
}

impl From<WitnessConditionType> for u8 {
    fn from(condition_type: WitnessConditionType) -> Self {
        condition_type as u8
    }
}
