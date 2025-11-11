use bitflags::bitflags;
use serde::{Deserialize, Serialize};

bitflags! {
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
    pub struct CallFlags: u8 {
        const NONE = 0b0000_0000;
        const READ_STATES = 0b0000_0001;
        const WRITE_STATES = 0b0000_0010;
        const ALLOW_CALL = 0b0000_0100;
        const ALLOW_NOTIFY = 0b0000_1000;
        const STATES = Self::READ_STATES.bits() | Self::WRITE_STATES.bits();
        const READ_ONLY = Self::READ_STATES.bits() | Self::ALLOW_CALL.bits();
        const ALL = Self::STATES.bits() | Self::ALLOW_CALL.bits() | Self::ALLOW_NOTIFY.bits();
    }
}

impl Default for CallFlags {
    fn default() -> Self {
        CallFlags::NONE
    }
}
