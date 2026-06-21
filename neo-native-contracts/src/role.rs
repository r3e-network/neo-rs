//! Role enum matching C# `Neo.SmartContract.Native.Role`.
//!
//! `Role` enumerates the consensus-relevant roles a committee member
//! can be designated for. The currently-defined roles are:
//!
//! - [`Role::StateValidator`] — validators for the dBFT state
//!   transition.
//! - [`Role::Oracle`] — oracle nodes (used by the oracle service).
//! - [`Role::NeoFsAlphabetNode`] — NeoFS alphabet nodes.
//! - [`Role::P2PNotary`] — P2P notary nodes.

use serde::{Deserialize, Serialize};

/// Role that a committee public key can be designated for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Role {
    /// State-validator role (dBFT).
    StateValidator,
    /// Oracle-node role.
    Oracle,
    /// NeoFS alphabet-node role.
    NeoFsAlphabetNode,
    /// P2P notary-node role.
    P2PNotary,
}

impl Role {
    /// Parses the byte representation used by the C# native-contract storage.
    pub fn from_byte(value: u8) -> Option<Self> {
        match value {
            4 => Some(Role::StateValidator),
            8 => Some(Role::Oracle),
            16 => Some(Role::NeoFsAlphabetNode),
            32 => Some(Role::P2PNotary),
            _ => None,
        }
    }

    /// Returns the integer index of the role (matches C# `Role` byte
    /// representation in the native-contract storage).
    pub fn as_byte(self) -> u8 {
        match self {
            Role::StateValidator => 4,
            Role::Oracle => 8,
            Role::NeoFsAlphabetNode => 16,
            Role::P2PNotary => 32,
        }
    }

    /// Returns the human-readable role name.
    pub fn as_str(self) -> &'static str {
        match self {
            Role::StateValidator => "StateValidator",
            Role::Oracle => "Oracle",
            Role::NeoFsAlphabetNode => "NeoFsAlphabetNode",
            Role::P2PNotary => "P2PNotary",
        }
    }
}

#[cfg(test)]
#[path = "tests/role.rs"]
mod tests;
