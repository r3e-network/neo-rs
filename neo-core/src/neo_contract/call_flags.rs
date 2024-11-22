use std::ops::BitOr;
use bitflags::bitflags;
use serde::{Serialize, Deserialize};

/// Represents the operations allowed when a contract is called.
bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
    pub struct CallFlags: u8 {
        /// No flag is set.
        const NONE = 0;

        /// Indicates that the called contract is allowed to read states.
        const READ_STATES = 0b00000001;

        /// Indicates that the called contract is allowed to write states.
        const WRITE_STATES = 0b00000010;

        /// Indicates that the called contract is allowed to call another contract.
        const ALLOW_CALL = 0b00000100;

        /// Indicates that the called contract is allowed to send notifications.
        const ALLOW_NOTIFY = 0b00001000;

        /// Indicates that the called contract is allowed to read or write states.
        const STATES = Self::READ_STATES.bits() | Self::WRITE_STATES.bits();

        /// Indicates that the called contract is allowed to read states or call another contract.
        const READ_ONLY = Self::READ_STATES.bits() | Self::ALLOW_CALL.bits();

        /// All flags are set.
        const ALL = Self::STATES.bits() | Self::ALLOW_CALL.bits() | Self::ALLOW_NOTIFY.bits();
    }
}

impl Default for CallFlags {
    fn default() -> Self {
        CallFlags::NONE
    }
}

impl BitOr for CallFlags {
    type Output = ();

    fn bitor(self, rhs: Self) -> Self::Output {
        self | rhs;
    }
}

impl Serialize for CallFlags {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u8(self.bits())
    }
}

impl<'de> Deserialize<'de> for CallFlags {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bits = u8::deserialize(deserializer)?;
        CallFlags::from_bits(bits).ok_or_else(|| serde::de::Error::custom("Invalid CallFlags value"))
    }
}
