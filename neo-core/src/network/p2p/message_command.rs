//! Message command identifiers (mirrors `Neo.Network.P2P.MessageCommand`).

use crate::NetworkError;
use std::net::SocketAddr;

neo_primitives::p2p_message_command! {
    /// Neo message command (single-byte discriminator).
    #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
    pub MessageCommand {
        error = NetworkError;
        parse_error = |other| NetworkError::ProtocolViolation {
                peer: SocketAddr::from(([0, 0, 0, 0], 0)),
                violation: format!("Unknown message command: {other}"),
            };
        from_byte = result;
        extended_aliases = true;
    }
}

neo_primitives::__p2p_message_command_compression_impl!(pub(crate) MessageCommand);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protocol_enum_guard_preserves_unknown_message_command_bytes() {
        let command = MessageCommand::from_byte(0x99).unwrap();
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
    fn compression_whitelist_matches_protocol_commands() {
        let compressible = [
            MessageCommand::Block,
            MessageCommand::Extensible,
            MessageCommand::Transaction,
            MessageCommand::Headers,
            MessageCommand::Addr,
            MessageCommand::MerkleBlock,
            MessageCommand::FilterLoad,
            MessageCommand::FilterAdd,
        ];

        for command in compressible {
            assert!(command.allows_compression(), "{command:?}");
        }

        assert!(!MessageCommand::Ping.allows_compression());
        assert!(!MessageCommand::Unknown(0x99).allows_compression());
    }

    #[test]
    fn extended_parse_aliases_preserve_legacy_unknown_commands() {
        assert_eq!(
            MessageCommand::parse_str("versionwithpayload").unwrap(),
            MessageCommand::Unknown(0x55)
        );
        assert_eq!(
            MessageCommand::parse_str("extendedc0").unwrap(),
            MessageCommand::Unknown(0xc0)
        );
    }
}
