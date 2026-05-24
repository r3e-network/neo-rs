//! Message flag definitions (mirrors `Neo.Network.P2P.MessageFlags`).

use crate::NetworkError;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Message flags applied to the network payload header.
///
/// The C# implementation treats this as a `[Flags]` enum, so we preserve the raw
/// byte rather than rejecting unknown combinations. Future protocol extensions
/// can therefore add bits without breaking the Rust node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageFlags(u8);

impl MessageFlags {
    /// No flags are set.
    pub const NONE: MessageFlags = MessageFlags(0x00);
    /// The payload is compressed.
    pub const COMPRESSED: MessageFlags = MessageFlags(0x01);

    /// Converts the flags to their byte representation.
    pub fn to_byte(self) -> u8 {
        self.0
    }

    /// Alias for [`Self::to_byte`]; retained for backward compatibility.
    pub fn as_byte(self) -> u8 {
        self.to_byte()
    }

    /// Parses the flags from their byte representation.
    pub fn from_byte(byte: u8) -> Result<Self, NetworkError> {
        if byte & !Self::COMPRESSED.0 != 0 {
            tracing::warn!(
                target: "neo",
                "message flags include unknown bits (0x{:02x}); preserving raw value",
                byte
            );
        }
        Ok(Self(byte))
    }

    /// Returns `true` when the compressed flag is set.
    pub fn is_compressed(self) -> bool {
        self.0 & Self::COMPRESSED.0 != 0
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_enum_guard_preserves_unknown_message_flag_bits() {
        let unknown = MessageFlags::from_byte(0x80).unwrap();
        assert_eq!(unknown.to_byte(), 0x80);
        assert_eq!(unknown.as_byte(), 0x80);
        assert!(!unknown.is_compressed());

        let combined = MessageFlags::from_byte(0x81).unwrap();
        assert_eq!(combined.to_byte(), 0x81);
        assert!(combined.is_compressed());

        let serialized = serde_json::to_string(&combined).unwrap();
        assert_eq!(serialized, "129");
        let deserialized: MessageFlags = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.to_byte(), 0x81);
    }
}
