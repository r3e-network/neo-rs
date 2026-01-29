//! Change view reason - Why validators request a view change.
//!
//! This module provides the `ChangeViewReason` enum, which indicates why a
//! validator is requesting a view change in the dBFT consensus protocol.
//!
//! ## Overview
//!
//! When the primary (speaker) fails or misbehaves, validators trigger a view
//! change to select a new primary. The reason helps diagnose consensus issues.
//!
//! ## Reasons
//!
//! | Reason | Description |
//! |--------|-------------|
//! | `Timeout` | No `PrepareRequest` received in time |
//! | `ChangeAgreement` | Agreed with other validators to change |
//! | `TxNotFound` | Required transaction not found |
//! | `TxRejectedByPolicy` | Transaction violates policy |
//! | `TxInvalid` | Transaction failed validation |
//! | `BlockRejectedByPolicy` | Block violates policy rules |
//!
//! ## Example
//!
//! ```rust
//! use neo_consensus::ChangeViewReason;
//!
//! let reason = ChangeViewReason::Timeout;
//! assert_eq!(reason.to_byte(), 0x0);
//! assert_eq!(reason.to_string(), "Timeout");
//! ```

use serde::{Deserialize, Serialize};

/// Change view reason enum matching C# `ChangeViewReason` exactly
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[repr(u8)]
pub enum ChangeViewReason {
    /// Timeout occurred - no consensus reached within the time limit
    #[default]
    Timeout = 0x0,
    /// Change agreement - validators agreed to change view
    ChangeAgreement = 0x1,
    /// Transaction not found - a required transaction is missing
    TxNotFound = 0x2,
    /// Transaction rejected by policy - transaction violates policy rules
    TxRejectedByPolicy = 0x3,
    /// Transaction invalid - transaction failed validation
    TxInvalid = 0x4,
    /// Block rejected by policy - proposed block violates policy rules
    BlockRejectedByPolicy = 0x5,
}

impl ChangeViewReason {
    /// Converts from byte value
    #[must_use]
    pub const fn from_byte(value: u8) -> Option<Self> {
        match value {
            0x0 => Some(Self::Timeout),
            0x1 => Some(Self::ChangeAgreement),
            0x2 => Some(Self::TxNotFound),
            0x3 => Some(Self::TxRejectedByPolicy),
            0x4 => Some(Self::TxInvalid),
            0x5 => Some(Self::BlockRejectedByPolicy),
            _ => None,
        }
    }

    /// Converts to byte value
    #[must_use]
    pub const fn to_byte(self) -> u8 {
        self as u8
    }

    /// Returns the string representation
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "Timeout",
            Self::ChangeAgreement => "ChangeAgreement",
            Self::TxNotFound => "TxNotFound",
            Self::TxRejectedByPolicy => "TxRejectedByPolicy",
            Self::TxInvalid => "TxInvalid",
            Self::BlockRejectedByPolicy => "BlockRejectedByPolicy",
        }
    }
}

impl std::fmt::Display for ChangeViewReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_change_view_reason_values() {
        assert_eq!(ChangeViewReason::Timeout as u8, 0x0);
        assert_eq!(ChangeViewReason::ChangeAgreement as u8, 0x1);
        assert_eq!(ChangeViewReason::TxNotFound as u8, 0x2);
        assert_eq!(ChangeViewReason::TxRejectedByPolicy as u8, 0x3);
        assert_eq!(ChangeViewReason::TxInvalid as u8, 0x4);
        assert_eq!(ChangeViewReason::BlockRejectedByPolicy as u8, 0x5);
    }

    #[test]
    fn test_change_view_reason_from_byte() {
        assert_eq!(
            ChangeViewReason::from_byte(0x0),
            Some(ChangeViewReason::Timeout)
        );
        assert_eq!(
            ChangeViewReason::from_byte(0x2),
            Some(ChangeViewReason::TxNotFound)
        );
        assert_eq!(ChangeViewReason::from_byte(0x99), None);
    }

    #[test]
    fn test_change_view_reason_roundtrip() {
        for reason in [
            ChangeViewReason::Timeout,
            ChangeViewReason::ChangeAgreement,
            ChangeViewReason::TxNotFound,
            ChangeViewReason::TxRejectedByPolicy,
            ChangeViewReason::TxInvalid,
            ChangeViewReason::BlockRejectedByPolicy,
        ] {
            let byte = reason.to_byte();
            let recovered = ChangeViewReason::from_byte(byte);
            assert_eq!(recovered, Some(reason));
        }
    }

    #[test]
    fn test_change_view_reason_default() {
        assert_eq!(ChangeViewReason::default(), ChangeViewReason::Timeout);
    }

    #[test]
    fn test_change_view_reason_display() {
        assert_eq!(ChangeViewReason::Timeout.to_string(), "Timeout");
        assert_eq!(ChangeViewReason::TxNotFound.to_string(), "TxNotFound");
    }
}
