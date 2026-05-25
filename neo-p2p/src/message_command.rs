//! Message command identifiers (mirrors `Neo.Network.P2P.MessageCommand`).

use crate::P2PError;

neo_primitives::p2p_message_command! {
    /// Neo message command (single-byte discriminator).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub MessageCommand {
        error = P2PError;
        parse_error = |other| P2PError::protocol_error(format!("Unknown message command: {other}"));
        from_byte = infallible;
        extended_aliases = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_command_byte_values() {
        assert_eq!(MessageCommand::Version.to_byte(), 0x00);
        assert_eq!(MessageCommand::Verack.to_byte(), 0x01);
        assert_eq!(MessageCommand::Transaction.to_byte(), 0x2b);
        assert_eq!(MessageCommand::Block.to_byte(), 0x2c);
        assert_eq!(MessageCommand::Extensible.to_byte(), 0x2e);
    }

    #[test]
    fn test_message_command_from_byte() {
        assert_eq!(MessageCommand::from_byte(0x00), MessageCommand::Version);
        assert_eq!(MessageCommand::from_byte(0x2b), MessageCommand::Transaction);
        assert_eq!(
            MessageCommand::from_byte(0x99),
            MessageCommand::Unknown(0x99)
        );
    }

    #[test]
    fn protocol_enum_guard_preserves_unknown_message_command_bytes() {
        let command = MessageCommand::from_byte(0x99);
        assert_eq!(command, MessageCommand::Unknown(0x99));
        assert_eq!(command.to_byte(), 0x99);
        assert_eq!(command.as_byte(), 0x99);
        assert_eq!(command.as_str(), "unknown");
        assert!(!command.is_known());

        let serialized = serde_json::to_string(&command).unwrap();
        assert_eq!(serialized, "153");
        let deserialized: MessageCommand = serde_json::from_str(&serialized).unwrap();
        assert_eq!(deserialized, command);
    }

    #[test]
    fn test_message_command_as_str() {
        assert_eq!(MessageCommand::Version.as_str(), "version");
        assert_eq!(MessageCommand::Transaction.as_str(), "tx");
        assert_eq!(MessageCommand::Block.as_str(), "block");
    }

    #[test]
    fn test_message_command_parse_str() {
        assert_eq!(
            MessageCommand::parse_str("version").unwrap(),
            MessageCommand::Version
        );
        assert_eq!(
            MessageCommand::parse_str("tx").unwrap(),
            MessageCommand::Transaction
        );
        assert!(MessageCommand::parse_str("invalid").is_err());
        assert!(MessageCommand::parse_str("versionwithpayload").is_err());
    }

    #[test]
    fn test_message_command_is_known() {
        assert!(MessageCommand::Version.is_known());
        assert!(MessageCommand::Block.is_known());
        assert!(!MessageCommand::Unknown(0x99).is_known());
    }

    #[test]
    fn test_message_command_roundtrip() {
        for cmd in [
            MessageCommand::Version,
            MessageCommand::Verack,
            MessageCommand::Transaction,
            MessageCommand::Block,
            MessageCommand::Extensible,
        ] {
            let byte = cmd.to_byte();
            let recovered = MessageCommand::from_byte(byte);
            assert_eq!(cmd, recovered);
        }
    }
}
