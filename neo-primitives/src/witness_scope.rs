//! Implementation of `WitnessScope`, representing the scope of a witness.
//!
//! Matches C# `Neo.Network.P2P.Payloads.WitnessScope` exactly.

use serde::{Deserialize, Serialize};
use std::fmt;
use std::ops::{BitAnd, BitAndAssign, BitOr, BitOrAssign, Not};
use std::str::FromStr;

/// Represents the scope of a witness (matches C# `WitnessScope` `Flags` enum exactly).
///
/// This is a flags enum that defines the different scopes that can be applied to a witness,
/// controlling which contracts and operations the witness can authorize.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct WitnessScope(u8);

impl WitnessScope {
    /// Indicates that no contract was witnessed. Only sign the transaction.
    pub const NONE: Self = Self(0x00);

    /// Indicates that the calling contract must be the entry contract.
    pub const CALLED_BY_ENTRY: Self = Self(0x01);

    /// Custom hash for contract-specific.
    pub const CUSTOM_CONTRACTS: Self = Self(0x10);

    /// Custom pubkey for group members.
    pub const CUSTOM_GROUPS: Self = Self(0x20);

    /// Indicates that the current context must satisfy the specified rules.
    pub const WITNESS_RULES: Self = Self(0x40);

    /// Global scope allows this witness in all contexts (default Neo 2 behavior).
    /// This cannot be combined with other flags.
    pub const GLOBAL: Self = Self(0x80);

    // C# naming convention aliases
    #[allow(non_upper_case_globals)]
    pub const CalledByEntry: Self = Self::CALLED_BY_ENTRY;
    #[allow(non_upper_case_globals)]
    pub const None: Self = Self::NONE;
    #[allow(non_upper_case_globals)]
    pub const Global: Self = Self::GLOBAL;
    #[allow(non_upper_case_globals)]
    pub const CustomContracts: Self = Self::CUSTOM_CONTRACTS;
    #[allow(non_upper_case_globals)]
    pub const CustomGroups: Self = Self::CUSTOM_GROUPS;
    #[allow(non_upper_case_globals)]
    pub const WitnessRules: Self = Self::WITNESS_RULES;

    /// Checks if this scope has the specified flag.
    #[must_use]
    pub const fn has_flag(self, flag: Self) -> bool {
        if flag.0 == 0 {
            return self.0 == 0;
        }
        (self.0 & flag.0) == flag.0
    }

    /// Checks if this scope contains the specified flag (alias for `has_flag`).
    #[must_use]
    pub fn contains(self, flag: Self) -> bool {
        self.has_flag(flag)
    }

    /// Combines this scope with another scope using bitwise OR.
    #[must_use]
    pub const fn combine(self, other: Self) -> Self {
        Self(self.0 | other.0)
    }

    /// Returns the raw bit representation of the scope.
    #[must_use]
    pub const fn bits(self) -> u8 {
        self.0
    }

    /// Creates a scope from a raw bit representation.
    #[must_use]
    pub fn from_bits(bits: u8) -> Option<Self> {
        Self::from_byte(bits)
    }

    /// Returns true if the scope shares any flags with `other`.
    #[must_use]
    pub const fn intersects(self, other: Self) -> bool {
        self.0 & other.0 != 0
    }

    /// Creates a `WitnessScope` from a byte value.
    #[must_use]
    pub const fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x00 => Some(Self::NONE),
            0x01 => Some(Self::CALLED_BY_ENTRY),
            0x10 => Some(Self::CUSTOM_CONTRACTS),
            0x20 => Some(Self::CUSTOM_GROUPS),
            0x40 => Some(Self::WITNESS_RULES),
            0x80 => Some(Self::GLOBAL),
            _ => {
                let valid_flags = 0x01 | 0x10 | 0x20 | 0x40 | 0x80;
                if (value & !valid_flags) == 0 {
                    if (value & 0x80) != 0 && value != 0x80 {
                        Option::None
                    } else {
                        Some(Self(value))
                    }
                } else {
                    Option::None
                }
            }
        }
    }

    /// Converts the `WitnessScope` to a byte value.
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        self.0
    }

    /// Validates that the scope combination is valid.
    #[must_use]
    pub fn is_valid(self) -> bool {
        let value = self.0;

        if self.has_flag(Self::GLOBAL) && value != Self::GLOBAL.0 {
            return false;
        }

        let valid_flags = Self::CALLED_BY_ENTRY.0
            | Self::CUSTOM_CONTRACTS.0
            | Self::CUSTOM_GROUPS.0
            | Self::WITNESS_RULES.0
            | Self::GLOBAL.0;

        (value & !valid_flags) == 0
    }
}

impl Default for WitnessScope {
    fn default() -> Self {
        Self::NONE
    }
}

impl FromStr for WitnessScope {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            return Ok(Self::NONE);
        }

        let mut scope = Self::NONE;
        let mut has_parts = false;
        for part in trimmed
            .split(['|', ','])
            .map(str::trim)
            .filter(|p| !p.is_empty())
        {
            has_parts = true;
            let flag = match part.to_ascii_lowercase().as_str() {
                "none" => Self::NONE,
                "calledbyentry" => Self::CALLED_BY_ENTRY,
                "customcontracts" => Self::CUSTOM_CONTRACTS,
                "customgroups" => Self::CUSTOM_GROUPS,
                "witnessrules" => Self::WITNESS_RULES,
                "global" => Self::GLOBAL,
                other => {
                    return Err(format!("Unknown witness scope: {other}"));
                }
            };

            if flag == Self::GLOBAL && scope != Self::NONE {
                return Err("Global scope cannot be combined with other flags".to_string());
            }
            if scope == Self::GLOBAL && flag != Self::GLOBAL {
                return Err("Global scope cannot be combined with other flags".to_string());
            }

            scope |= flag;
        }

        if !has_parts {
            return Ok(Self::NONE);
        }

        if !scope.is_valid() {
            return Err(format!("Invalid witness scope combination: {trimmed}"));
        }

        Ok(scope)
    }
}

impl fmt::Display for WitnessScope {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::NONE => write!(f, "None"),
            Self::CALLED_BY_ENTRY => write!(f, "CalledByEntry"),
            Self::CUSTOM_CONTRACTS => write!(f, "CustomContracts"),
            Self::CUSTOM_GROUPS => write!(f, "CustomGroups"),
            Self::WITNESS_RULES => write!(f, "WitnessRules"),
            Self::GLOBAL => write!(f, "Global"),
            _ => {
                let mut parts = Vec::new();
                if self.has_flag(Self::CALLED_BY_ENTRY) {
                    parts.push("CalledByEntry");
                }
                if self.has_flag(Self::CUSTOM_CONTRACTS) {
                    parts.push("CustomContracts");
                }
                if self.has_flag(Self::CUSTOM_GROUPS) {
                    parts.push("CustomGroups");
                }
                if self.has_flag(Self::WITNESS_RULES) {
                    parts.push("WitnessRules");
                }
                if self.has_flag(Self::GLOBAL) {
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

impl BitOr for WitnessScope {
    type Output = Self;
    fn bitor(self, rhs: Self) -> Self::Output {
        Self(self.0 | rhs.0)
    }
}

impl BitOrAssign for WitnessScope {
    fn bitor_assign(&mut self, rhs: Self) {
        self.0 |= rhs.0;
    }
}

impl BitAnd for WitnessScope {
    type Output = Self;
    fn bitand(self, rhs: Self) -> Self::Output {
        Self(self.0 & rhs.0)
    }
}

impl BitAndAssign for WitnessScope {
    fn bitand_assign(&mut self, rhs: Self) {
        self.0 &= rhs.0;
    }
}

impl Not for WitnessScope {
    type Output = Self;
    fn not(self) -> Self::Output {
        Self(!self.0)
    }
}

/// Error type for invalid `WitnessScope` conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct InvalidWitnessScopeError(pub u8);

impl std::fmt::Display for InvalidWitnessScopeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "Invalid witness scope byte: 0x{:02X}. Valid values are 0x00, 0x01, 0x10, 0x20, 0x40, 0x80, or valid combinations.",
            self.0
        )
    }
}

impl std::error::Error for InvalidWitnessScopeError {}

impl TryFrom<u8> for WitnessScope {
    type Error = InvalidWitnessScopeError;

    /// Converts a byte to `WitnessScope`, returning an error for invalid values.
    ///
    /// # Security Note
    /// This method properly rejects invalid scope bytes instead of silently
    /// falling back to NONE, which could bypass witness restrictions.
    fn try_from(value: u8) -> Result<Self, Self::Error> {
        Self::from_byte(value).ok_or(InvalidWitnessScopeError(value))
    }
}

impl WitnessScope {
    /// Converts a byte to `WitnessScope`, falling back to NONE for invalid values.
    ///
    /// # Security Warning
    /// This method silently converts invalid values to NONE, which could bypass
    /// witness restrictions. Use `TryFrom<u8>` or `from_byte()` for proper error handling.
    ///
    /// # Deprecated
    /// This method is deprecated. Use `WitnessScope::try_from(value)` instead.
    #[deprecated(
        since = "0.7.1",
        note = "Use TryFrom<u8> or from_byte() instead. This method silently converts invalid values to NONE, which is a security risk."
    )]
    #[must_use]
    pub fn from_u8_lossy(value: u8) -> Self {
        Self::from_byte(value).unwrap_or_else(|| {
            tracing::warn!(
                "Invalid WitnessScope byte 0x{:02X} silently converted to NONE. Use TryFrom<u8> instead.",
                value
            );
            Self::NONE
        })
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
        assert_eq!(WitnessScope::NONE.to_byte(), 0x00);
        assert_eq!(WitnessScope::CALLED_BY_ENTRY.to_byte(), 0x01);
        assert_eq!(WitnessScope::CUSTOM_CONTRACTS.to_byte(), 0x10);
        assert_eq!(WitnessScope::CUSTOM_GROUPS.to_byte(), 0x20);
        assert_eq!(WitnessScope::WITNESS_RULES.to_byte(), 0x40);
        assert_eq!(WitnessScope::GLOBAL.to_byte(), 0x80);
    }

    #[test]
    fn test_witness_scope_has_flag() {
        let scope = WitnessScope::CALLED_BY_ENTRY;
        assert!(scope.has_flag(WitnessScope::CALLED_BY_ENTRY));
        assert!(!scope.has_flag(WitnessScope::CUSTOM_CONTRACTS));

        let combined = WitnessScope::CALLED_BY_ENTRY.combine(WitnessScope::CUSTOM_CONTRACTS);
        assert!(combined.has_flag(WitnessScope::CALLED_BY_ENTRY));
        assert!(combined.has_flag(WitnessScope::CUSTOM_CONTRACTS));
    }

    #[test]
    fn test_witness_scope_from_byte() {
        assert_eq!(WitnessScope::from_byte(0x00), Some(WitnessScope::NONE));
        assert_eq!(
            WitnessScope::from_byte(0x01),
            Some(WitnessScope::CALLED_BY_ENTRY)
        );
        assert_eq!(WitnessScope::from_byte(0x80), Some(WitnessScope::GLOBAL));
        assert_eq!(WitnessScope::from_byte(0xFF), Option::None);
    }

    #[test]
    fn test_witness_scope_is_valid() {
        assert!(WitnessScope::NONE.is_valid());
        assert!(WitnessScope::CALLED_BY_ENTRY.is_valid());
        assert!(WitnessScope::GLOBAL.is_valid());

        // Combined flags (non-global) should be valid
        let combined = WitnessScope::CALLED_BY_ENTRY | WitnessScope::CUSTOM_CONTRACTS;
        assert!(combined.is_valid());
    }

    #[test]
    fn test_witness_scope_display() {
        assert_eq!(format!("{}", WitnessScope::NONE), "None");
        assert_eq!(
            format!("{}", WitnessScope::CALLED_BY_ENTRY),
            "CalledByEntry"
        );
        assert_eq!(format!("{}", WitnessScope::GLOBAL), "Global");
    }

    #[test]
    fn test_witness_scope_from_str() {
        assert_eq!(WitnessScope::from_str("None").unwrap(), WitnessScope::NONE);
        assert_eq!(
            WitnessScope::from_str("CalledByEntry").unwrap(),
            WitnessScope::CALLED_BY_ENTRY
        );
        assert_eq!(
            WitnessScope::from_str("Global").unwrap(),
            WitnessScope::GLOBAL
        );
        assert!(WitnessScope::from_str("Invalid").is_err());
    }

    #[test]
    fn test_witness_scope_conversions() {
        let scope = WitnessScope::CALLED_BY_ENTRY;
        let byte_value: u8 = scope.into();
        assert_eq!(byte_value, 0x01);
        // Use TryFrom instead of From for safe conversion (returns error for invalid values)
        let converted_scope = WitnessScope::try_from(byte_value).unwrap();
        assert_eq!(converted_scope, scope);
    }

    #[test]
    fn test_witness_scope_default() {
        assert_eq!(WitnessScope::default(), WitnessScope::NONE);
    }

    #[test]
    fn test_witness_scope_bitwise_ops() {
        let mut scope = WitnessScope::CALLED_BY_ENTRY;
        scope |= WitnessScope::CUSTOM_CONTRACTS;
        assert!(scope.has_flag(WitnessScope::CALLED_BY_ENTRY));
        assert!(scope.has_flag(WitnessScope::CUSTOM_CONTRACTS));

        let masked = scope & WitnessScope::CALLED_BY_ENTRY;
        assert_eq!(masked, WitnessScope::CALLED_BY_ENTRY);
    }
}
