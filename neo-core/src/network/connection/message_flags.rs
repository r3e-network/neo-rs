use bitflags::bitflags;

bitflags! {
    /// Represents the flags of a message.
    pub struct MessageFlags: u8 {
        /// No flag is set for the message.
        const NONE = 0;

        /// Indicates that the message is compressed.
        const COMPRESSED = 1 << 0;
    }
}
