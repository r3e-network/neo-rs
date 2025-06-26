// Copyright (C) 2015-2025 The Neo Project.
//
// witness_scope.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

//! Implementation of WitnessScope, representing the scope of a witness.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Represents the scope of a witness (matches C# WitnessScope [Flags] enum exactly).
///
/// This is a flags enum that defines the different scopes that can be applied to a witness,
/// controlling which contracts and operations the witness can authorize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WitnessScope(u8);

impl WitnessScope {
    /// Indicates that no contract was witnessed. Only sign the transaction.
    pub const None: WitnessScope = WitnessScope(0x00);

    /// Indicates that the calling contract must be the entry contract.
    /// The witness/permission/signature given on first invocation will automatically expire if entering deeper internal invokes.
    /// This can be the default safe choice for native NEO/GAS (previously used on Neo 2 as "attach" mode).
    pub const CalledByEntry: WitnessScope = WitnessScope(0x01);

    /// Custom hash for contract-specific.
    pub const CustomContracts: WitnessScope = WitnessScope(0x10);

    /// Custom pubkey for group members.
    pub const CustomGroups: WitnessScope = WitnessScope(0x20);

    /// Indicates that the current context must satisfy the specified rules.
    pub const WitnessRules: WitnessScope = WitnessScope(0x40);

    /// Global scope allows this witness in all contexts (default Neo 2 behavior).
    /// This cannot be combined with other flags.
    pub const Global: WitnessScope = WitnessScope(0x80);
}

impl WitnessScope {
    /// Checks if this scope has the specified flag (matches C# HasFlag exactly).
    ///
    /// # Arguments
    ///
    /// * `flag` - The flag to check for
    ///
    /// # Returns
    ///
    /// true if the flag is set, false otherwise
    pub fn has_flag(self, flag: WitnessScope) -> bool {
        self.0 & flag.0 != 0
    }

    /// Checks if this scope contains the specified flag (alias for has_flag).
    ///
    /// # Arguments
    ///
    /// * `flag` - The flag to check for
    ///
    /// # Returns
    ///
    /// true if the flag is set, false otherwise
    pub fn contains(self, flag: WitnessScope) -> bool {
        self.has_flag(flag)
    }

    /// Combines this scope with another scope using bitwise OR (matches C# | operator exactly).
    ///
    /// # Arguments
    ///
    /// * `other` - The other scope to combine with
    ///
    /// # Returns
    ///
    /// The combined scope
    pub fn combine(self, other: WitnessScope) -> Self {
        WitnessScope(self.0 | other.0)
    }

    /// Creates a WitnessScope from a byte value.
    ///
    /// # Arguments
    ///
    /// * `value` - The byte value to convert
    ///
    /// # Returns
    ///
    /// Some(WitnessScope) if the value is valid, None otherwise
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(WitnessScope::None),
            0x01 => Some(WitnessScope::CalledByEntry),
            0x10 => Some(WitnessScope::CustomContracts),
            0x20 => Some(WitnessScope::CustomGroups),
            0x40 => Some(WitnessScope::WitnessRules),
            0x80 => Some(WitnessScope::Global),
            _ => {
                // Production-ready flag combination validation (matches C# WitnessScope validation exactly)
                // This implements the C# logic: handling valid flag combinations

                // 1. Check if it's a valid combination of flags (production validation)
                let valid_flags = 0x01 | 0x10 | 0x20 | 0x40 | 0x80;
                if (value & !valid_flags) == 0 {
                    // 2. Check if Global is combined with other flags (production rule enforcement)
                    if (value & 0x80) != 0 && value != 0x80 {
                        // Global cannot be combined with other flags (matches C# validation exactly)
                        None
                    } else {
                        // 3. Valid flag combination (production acceptance)
                        Some(WitnessScope(value))
                    }
                } else {
                    // 4. Invalid flags detected (production rejection)
                    None
                }
            }
        }
    }

    /// Converts the WitnessScope to a byte value (matches C# (byte)scope exactly).
    ///
    /// # Returns
    ///
    /// The byte representation of the scope
    pub fn to_byte(self) -> u8 {
        self.0
    }

    /// Validates that the scope combination is valid (matches C# validation exactly).
    ///
    /// # Returns
    ///
    /// true if the scope is valid, false otherwise
    pub fn is_valid(self) -> bool {
        let value = self.0;

        // Global scope cannot be combined with other flags (matches C# validation)
        if self.has_flag(WitnessScope::Global) && value != WitnessScope::Global.0 {
            return false;
        }

        // Check that only valid flags are set (matches C# validation)
        let valid_flags = WitnessScope::CalledByEntry.0
            | WitnessScope::CustomContracts.0
            | WitnessScope::CustomGroups.0
            | WitnessScope::WitnessRules.0
            | WitnessScope::Global.0;

        (value & !valid_flags) == 0
    }
}

impl Default for WitnessScope {
    fn default() -> Self {
        WitnessScope::None
    }
}

impl fmt::Display for WitnessScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            WitnessScope::None => write!(f, "None"),
            WitnessScope::CalledByEntry => write!(f, "CalledByEntry"),
            WitnessScope::CustomContracts => write!(f, "CustomContracts"),
            WitnessScope::CustomGroups => write!(f, "CustomGroups"),
            WitnessScope::WitnessRules => write!(f, "WitnessRules"),
            WitnessScope::Global => write!(f, "Global"),
            _ => {
                // Handle combined flags by showing individual components
                let mut parts = Vec::new();
                if self.has_flag(WitnessScope::CalledByEntry) {
                    parts.push("CalledByEntry");
                }
                if self.has_flag(WitnessScope::CustomContracts) {
                    parts.push("CustomContracts");
                }
                if self.has_flag(WitnessScope::CustomGroups) {
                    parts.push("CustomGroups");
                }
                if self.has_flag(WitnessScope::WitnessRules) {
                    parts.push("WitnessRules");
                }
                if self.has_flag(WitnessScope::Global) {
                    parts.push("Global");
                }
                if parts.is_empty() {
                    write!(f, "None")
                } else {
                    write!(f, "{}", parts.join(" | "))
                }
            }
        }
    }
}

impl From<u8> for WitnessScope {
    fn from(value: u8) -> Self {
        Self::from_byte(value).unwrap_or(WitnessScope::None)
    }
}

impl From<WitnessScope> for u8 {
    fn from(scope: WitnessScope) -> Self {
        scope.to_byte()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_witness_scope_values() {
        assert_eq!(WitnessScope::None.to_byte(), 0x00);
        assert_eq!(WitnessScope::CalledByEntry.to_byte(), 0x01);
        assert_eq!(WitnessScope::CustomContracts.to_byte(), 0x10);
        assert_eq!(WitnessScope::CustomGroups.to_byte(), 0x20);
        assert_eq!(WitnessScope::WitnessRules.to_byte(), 0x40);
        assert_eq!(WitnessScope::Global.to_byte(), 0x80);
    }

    #[test]
    fn test_witness_scope_has_flag() {
        let scope = WitnessScope::CalledByEntry;
        assert!(scope.has_flag(WitnessScope::CalledByEntry));
        assert!(!scope.has_flag(WitnessScope::CustomContracts));

        let combined = WitnessScope::CalledByEntry.combine(WitnessScope::CustomContracts);
        assert!(combined.has_flag(WitnessScope::CalledByEntry));
        assert!(combined.has_flag(WitnessScope::CustomContracts));
    }

    #[test]
    fn test_witness_scope_from_byte() {
        assert_eq!(WitnessScope::from_byte(0x00), Some(WitnessScope::None));
        assert_eq!(
            WitnessScope::from_byte(0x01),
            Some(WitnessScope::CalledByEntry)
        );
        assert_eq!(WitnessScope::from_byte(0x80), Some(WitnessScope::Global));
        assert_eq!(WitnessScope::from_byte(0xFF), None);
    }

    #[test]
    fn test_witness_scope_is_valid() {
        assert!(WitnessScope::None.is_valid());
        assert!(WitnessScope::CalledByEntry.is_valid());
        assert!(WitnessScope::Global.is_valid());

        // Global cannot be combined with other flags
        let invalid_global = WitnessScope::from_byte(0x81); // Global + CalledByEntry
        if let Some(scope) = invalid_global {
            assert!(!scope.is_valid());
        }
    }

    #[test]
    fn test_witness_scope_display() {
        assert_eq!(format!("{}", WitnessScope::None), "None");
        assert_eq!(format!("{}", WitnessScope::CalledByEntry), "CalledByEntry");
        assert_eq!(format!("{}", WitnessScope::Global), "Global");
    }

    #[test]
    fn test_witness_scope_conversions() {
        let scope = WitnessScope::CalledByEntry;
        let byte_value: u8 = scope.into();
        assert_eq!(byte_value, 0x01);

        let converted_scope: WitnessScope = byte_value.into();
        assert_eq!(converted_scope, scope);
    }
}
