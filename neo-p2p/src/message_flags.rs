//! Message flag definitions (mirrors `Neo.Network.P2P.MessageFlags`).

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Message flags applied to the network payload header.
///
/// The C# implementation treats this as a `[Flags]` enum, so we preserve the raw
/// byte rather than rejecting unknown combinations. Future protocol extensions
/// can therefore add bits without breaking the Rust node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct MessageFlags(u8);

impl MessageFlags {
    /// No flags are set.
    pub const NONE: MessageFlags = MessageFlags(0x00);
    /// The payload is compressed.
    pub const COMPRESSED: MessageFlags = MessageFlags(0x01);

    /// Creates a new MessageFlags with the given raw value.
    pub const fn new(value: u8) -> Self {
        Self(value)
    }

    /// Converts the flags to their byte representation.
    pub fn to_byte(self) -> u8 {
        self.0
    }

    /// Alias for [`Self::to_byte`]; retained for backward compatibility.
    pub fn as_byte(self) -> u8 {
        self.to_byte()
    }

    /// Parses the flags from their byte representation.
    ///
    /// This method accepts any byte value, logging a warning for unknown bits
    /// but preserving them for forward compatibility.
    pub fn from_byte(byte: u8) -> Self {
        if byte & !Self::COMPRESSED.0 != 0 {
            tracing::warn!(
                target: "neo::p2p",
                "message flags include unknown bits (0x{:02x}); preserving raw value",
                byte
            );
        }
        Self(byte)
    }

    /// Returns `true` when the compressed flag is set.
    pub fn is_compressed(self) -> bool {
        self.0 & Self::COMPRESSED.0 != 0
    }

    /// Sets the compressed flag.
    pub fn set_compressed(&mut self, compressed: bool) {
        if compressed {
            self.0 |= Self::COMPRESSED.0;
        } else {
            self.0 &= !Self::COMPRESSED.0;
        }
    }

    /// Returns a new MessageFlags with the compressed flag set.
    pub fn with_compressed(mut self, compressed: bool) -> Self {
        self.set_compressed(compressed);
        self
    }
}

impl Serialize for MessageFlags {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_u8(self.to_byte())
    }
}

impl<'de> Deserialize<'de> for MessageFlags {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let value = u8::deserialize(deserializer)?;
        Ok(Self(value))
    }
}

impl std::fmt::Display for MessageFlags {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if self.0 == 0 {
            write!(f, "None")
        } else if self.is_compressed() {
            write!(f, "Compressed")
        } else {
            write!(f, "Flags(0x{:02x})", self.0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_flags_none() {
        let flags = MessageFlags::NONE;
        assert_eq!(flags.to_byte(), 0x00);
        assert!(!flags.is_compressed());
    }

    #[test]
    fn test_message_flags_compressed() {
        let flags = MessageFlags::COMPRESSED;
        assert_eq!(flags.to_byte(), 0x01);
        assert!(flags.is_compressed());
    }

    #[test]
    fn test_message_flags_from_byte() {
        let flags = MessageFlags::from_byte(0x01);
        assert!(flags.is_compressed());

        let flags = MessageFlags::from_byte(0x00);
        assert!(!flags.is_compressed());
    }

    #[test]
    fn test_message_flags_set_compressed() {
        let mut flags = MessageFlags::NONE;
        assert!(!flags.is_compressed());

        flags.set_compressed(true);
        assert!(flags.is_compressed());

        flags.set_compressed(false);
        assert!(!flags.is_compressed());
    }

    #[test]
    fn test_message_flags_with_compressed() {
        let flags = MessageFlags::NONE.with_compressed(true);
        assert!(flags.is_compressed());
    }

    #[test]
    fn test_message_flags_display() {
        assert_eq!(MessageFlags::NONE.to_string(), "None");
        assert_eq!(MessageFlags::COMPRESSED.to_string(), "Compressed");
    }

    #[test]
    fn test_message_flags_default() {
        let flags = MessageFlags::default();
        assert_eq!(flags, MessageFlags::NONE);
    }
}
