// Copyright (C) 2015-2025 The Neo Project.
//
// witness_condition_type.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use serde::{Deserialize, Serialize};

/// Represents the type of WitnessCondition.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
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

impl WitnessConditionType {
    /// Convert from byte value.
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::Boolean),
            0x01 => Some(Self::Not),
            0x02 => Some(Self::And),
            0x03 => Some(Self::Or),
            0x18 => Some(Self::ScriptHash),
            0x19 => Some(Self::Group),
            0x20 => Some(Self::CalledByEntry),
            0x28 => Some(Self::CalledByContract),
            0x29 => Some(Self::CalledByGroup),
            _ => None,
        }
    }

    /// Convert to byte value.
    pub fn to_byte(self) -> u8 {
        self as u8
    }
}
