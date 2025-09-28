//! CallFlags bitflags mirroring Neo.SmartContract.CallFlags from the C# codebase.
//! These flags describe the permissions granted to a contract call.

use bitflags::bitflags;

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
    pub const STATES: CallFlags = CallFlags::READ_STATES.union(CallFlags::WRITE_STATES);
    /// Combination of `READ_STATES` and `ALLOW_CALL` permissions.
    pub const READ_ONLY: CallFlags = CallFlags::READ_STATES.union(CallFlags::ALLOW_CALL);
    /// All available permissions.
    pub const ALL: CallFlags = CallFlags::STATES
        .union(CallFlags::ALLOW_CALL)
        .union(CallFlags::ALLOW_NOTIFY);
}
