//! Message flag definitions (mirrors `Neo.Network.P2P.MessageFlags`).

use crate::NetworkError;
use neo_primitives::protocol_message_flags;

protocol_message_flags! {
    /// Message flags applied to the network payload header.
    ///
    /// The C# implementation treats this as a `[Flags]` enum, so we preserve the raw
    /// byte rather than rejecting unknown combinations. Future protocol extensions
    /// can therefore add bits without breaking the Rust node.
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
    pub MessageFlags {
        warn_target = "neo";
        from_byte = from_byte_unchecked;
    }
}

impl MessageFlags {
    /// Parses the flags from their byte representation.
    pub fn from_byte(byte: u8) -> Result<Self, NetworkError> {
        Ok(Self::from_byte_unchecked(byte))
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
        assert_eq!(unknown.to_string(), "Flags(0x80)");
        assert_eq!(
            MessageFlags::from_bits(0x80)
                .expect("unknown bits retained")
                .to_byte(),
            0x80
        );

        let combined = MessageFlags::from_byte(0x81).unwrap();
        assert_eq!(combined.to_byte(), 0x81);
        assert!(combined.is_compressed());

        let serialized = serde_json::to_string(&combined).unwrap();
        assert_eq!(serialized, "129");
        let deserialized: MessageFlags = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized.to_byte(), 0x81);
    }

    #[test]
    fn default_and_display_match_p2p_api() {
        assert_eq!(MessageFlags::default(), MessageFlags::NONE);
        assert_eq!(MessageFlags::NONE.to_string(), "None");
        assert_eq!(MessageFlags::COMPRESSED.to_string(), "Compressed");
        assert_eq!(MessageFlags::from_byte(0x80).unwrap().to_string(), "Flags(0x80)");
    }
}
