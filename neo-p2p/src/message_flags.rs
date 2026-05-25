//! Message flag definitions (mirrors `Neo.Network.P2P.MessageFlags`).

use neo_primitives::protocol_message_flags;

protocol_message_flags! {
    /// Message flags applied to the network payload header.
    ///
    /// The C# implementation treats this as a `[Flags]` enum, so we preserve the raw
    /// byte rather than rejecting unknown combinations. Future protocol extensions
    /// can therefore add bits without breaking the Rust node.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub MessageFlags {
        warn_target = "neo::p2p";
        from_byte = from_byte;
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
    fn protocol_enum_guard_preserves_unknown_message_flag_bits() {
        let unknown = MessageFlags::from_byte(0x80);
        assert_eq!(unknown.to_byte(), 0x80);
        assert!(!unknown.is_compressed());
        assert_eq!(unknown.to_string(), "Flags(0x80)");

        let combined = MessageFlags::from_byte(0x81);
        assert_eq!(combined.to_byte(), 0x81);
        assert!(combined.is_compressed());

        let serialized = serde_json::to_string(&combined).unwrap();
        assert_eq!(serialized, "129");
        let deserialized: MessageFlags = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.to_byte(), 0x81);
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
