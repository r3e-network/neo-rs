use bitflags::bitflags;

bitflags! {
    /// Represents the triggers for running smart contracts.
    pub struct TriggerType: u8 {
        /// Indicate that the contract is triggered by the system to execute the OnPersist method of the native contracts.
        const ON_PERSIST = 0x01;

        /// Indicate that the contract is triggered by the system to execute the PostPersist method of the native contracts.
        const POST_PERSIST = 0x02;

        /// Indicates that the contract is triggered by the verification of a verifiable object.
        const VERIFICATION = 0x20;

        /// Indicates that the contract is triggered by the execution of transactions.
        const APPLICATION = 0x40;

        /// The combination of all system triggers.
        const SYSTEM = Self::ON_PERSIST.bits() | Self::POST_PERSIST.bits();

        /// The combination of all triggers.
        const ALL = Self::ON_PERSIST.bits() | Self::POST_PERSIST.bits() | Self::VERIFICATION.bits() | Self::APPLICATION.bits();
    }
}
