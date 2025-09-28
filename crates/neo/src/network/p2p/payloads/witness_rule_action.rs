// Copyright (C) 2015-2025 The Neo Project.
//
// witness_rule_action.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use serde::{Deserialize, Serialize};

/// Represents the action of a WitnessRule.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum WitnessRuleAction {
    /// Deny the witness.
    Deny = 0x00,

    /// Allow the witness.
    Allow = 0x01,
}

impl WitnessRuleAction {
    /// Convert from byte value.
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::Deny),
            0x01 => Some(Self::Allow),
            _ => None,
        }
    }

    /// Convert to byte value.
    pub fn to_byte(self) -> u8 {
        self as u8
    }

    /// Convert from string value.
    pub fn from_str(s: &str) -> Result<Self, String> {
        match s {
            "Deny" | "deny" => Ok(Self::Deny),
            "Allow" | "allow" => Ok(Self::Allow),
            _ => Err(format!("Invalid witness rule action: {}", s)),
        }
    }
}

impl std::fmt::Display for WitnessRuleAction {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let s = match self {
            Self::Deny => "Deny",
            Self::Allow => "Allow",
        };
        write!(f, "{}", s)
    }
}
