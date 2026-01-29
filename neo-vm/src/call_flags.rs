//! CallFlags - Permission flags for contract invocations.
//!
//! This module provides the `CallFlags` type, which defines the permissions
//! granted when one contract calls another, matching the C# Neo implementation.
//!
//! ## Flags
//!
//! | Flag | Value | Description |
//! |------|-------|-------------|
//! | `NONE` | 0x00 | No permissions granted |
//! | `READ_STATES` | 0x01 | Allowed to read contract states |
//! | `WRITE_STATES` | 0x02 | Allowed to write contract states |
//! | `ALLOW_CALL` | 0x04 | Allowed to call other contracts |
//! | `ALLOW_NOTIFY` | 0x08 | Allowed to emit notifications |
//!
//! ## Common Combinations
//!
//! | Combination | Flags |
//! |-------------|-------|
//! | `STATES` | `READ_STATES \| WRITE_STATES` |
//! | `READ_ONLY` | `READ_STATES \| ALLOW_CALL` |
//! | `ALL` | All flags combined |
//!
//! ## Example
//!
//! ```rust
//! use neo_vm::CallFlags;
//!
//! // Check if read permission is granted
//! let flags = CallFlags::READ_STATES | CallFlags::ALLOW_CALL;
//! assert!(flags.contains(CallFlags::READ_STATES));
//! assert!(!flags.contains(CallFlags::WRITE_STATES));
//!
//! // Use predefined combinations
//! let read_only = CallFlags::READ_ONLY;
//! ```

use bitflags::bitflags;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

bitflags! {
    /// Represents the operations allowed when a contract is invoked.
    #[derive(Default, Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub struct CallFlags: u8 {
        /// No flag is set.
        const NONE = 0b0000_0000;
        /// Indicates that the called contract is allowed to read states.
        const READ_STATES = 0b0000_0001;
        /// Indicates that the called contract is allowed to write states.
        const WRITE_STATES = 0b0000_0010;
        /// Indicates that the called contract is allowed to invoke another contract.
        const ALLOW_CALL = 0b0000_0100;
        /// Indicates that the called contract is allowed to publish notifications.
        const ALLOW_NOTIFY = 0b0000_1000;
    }
}

impl CallFlags {
    /// Combination of `READ_STATES` and `WRITE_STATES` permissions.
    pub const STATES: Self = Self::READ_STATES.union(Self::WRITE_STATES);
    /// Combination of `READ_STATES` and `ALLOW_CALL` permissions.
    pub const READ_ONLY: Self = Self::READ_STATES.union(Self::ALLOW_CALL);
    /// All available permissions.
    pub const ALL: Self = Self::STATES
        .union(Self::ALLOW_CALL)
        .union(Self::ALLOW_NOTIFY);
}

impl Serialize for CallFlags {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.bits())
    }
}

impl<'de> Deserialize<'de> for CallFlags {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        Self::from_bits(value)
            .ok_or_else(|| serde::de::Error::custom(format!("Invalid CallFlags value: {value}")))
    }
}
