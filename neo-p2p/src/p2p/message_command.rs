//! Message command identifiers (mirrors `Neo.Network.P2P.MessageCommand`).

use neo_primitives::NetworkError;
use std::net::SocketAddr;

/// Outbound write class for one message.
///
/// The class decides whether a message may share a single vectored write with
/// other messages (`Batchable`) or must be written in its own write window as
/// soon as the outbound queue reaches it (`Immediate`).
///
/// The command-level default below is deliberately conservative: only the
/// announcement/relay payloads are batchable, and every request or reply that
/// a peer can time keeps the immediate class. Call sites that answer a request
/// with an otherwise batchable command (a `getdata` answer carrying a block,
/// for example) must downgrade to `Immediate` explicitly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OutboundClass {
    /// Written in its own write window, in queue order.
    Immediate,
    /// May wait behind other batchable messages for at most the batch delay.
    Batchable,
}

impl OutboundClass {
    /// Returns true when the message must not be coalesced.
    pub fn is_immediate(self) -> bool {
        matches!(self, Self::Immediate)
    }

    /// Returns true when the message may be coalesced into a batch.
    pub fn is_batchable(self) -> bool {
        matches!(self, Self::Batchable)
    }
}

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

neo_primitives::__p2p_message_command_compression_impl!(pub MessageCommand);

impl MessageCommand {
    /// Commands that should only have one item queued at a time.
    pub const SINGLE_QUEUED_COMMANDS: [Self; 7] = [
        Self::Addr,
        Self::GetAddr,
        Self::GetBlocks,
        Self::GetHeaders,
        Self::Mempool,
        Self::Ping,
        Self::Pong,
    ];

    /// Commands that should be processed with high priority.
    pub const HIGH_PRIORITY_COMMANDS: [Self; 7] = [
        Self::Alert,
        Self::Extensible,
        Self::FilterAdd,
        Self::FilterClear,
        Self::FilterLoad,
        Self::GetAddr,
        Self::Mempool,
    ];

    /// Commands whose outbound messages may be coalesced into one bounded
    /// vectored write: inventory announcements and block/transaction/merkle
    /// relay payloads, which carry no response deadline of their own.
    ///
    /// Everything else is `Immediate`, including the handshake (`version`,
    /// `verack`), consensus payloads (`extensible`), liveness (`ping`, `pong`),
    /// teardown, every request (`getaddr`, `getheaders`, `getblocks`,
    /// `getblkbyidx`, `getdata`, `mempool`) and every reply (`addr`, `headers`,
    /// `notfound`, `reject`, `alert`), plus the stateful bloom-filter commands.
    /// Unknown commands keep the immediate class: a message this node does not
    /// understand must never be delayed behind a batch.
    pub const BATCHABLE_COMMANDS: [Self; 4] =
        [Self::Inv, Self::Transaction, Self::Block, Self::MerkleBlock];

    /// Returns true if this command should only have one item queued.
    pub fn is_single_queued(self) -> bool {
        Self::SINGLE_QUEUED_COMMANDS.contains(&self)
    }

    /// Returns true if this command should be processed with high priority.
    pub fn is_high_priority_queue(self) -> bool {
        Self::HIGH_PRIORITY_COMMANDS.contains(&self)
    }

    /// Returns true when this command may share a vectored write.
    pub fn is_batchable(self) -> bool {
        Self::BATCHABLE_COMMANDS.contains(&self)
    }

    /// Returns true when this command must be written in its own write window.
    pub fn is_immediate(self) -> bool {
        !self.is_batchable()
    }

    /// Returns the outbound write class for this command.
    pub fn outbound_class(self) -> OutboundClass {
        if self.is_batchable() {
            OutboundClass::Batchable
        } else {
            OutboundClass::Immediate
        }
    }
}

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

    #[test]
    fn outbound_queue_single_command_set_matches_remote_node_policy() {
        assert_eq!(
            MessageCommand::SINGLE_QUEUED_COMMANDS,
            [
                MessageCommand::Addr,
                MessageCommand::GetAddr,
                MessageCommand::GetBlocks,
                MessageCommand::GetHeaders,
                MessageCommand::Mempool,
                MessageCommand::Ping,
                MessageCommand::Pong,
            ]
        );

        for command in MessageCommand::SINGLE_QUEUED_COMMANDS {
            assert!(command.is_single_queued(), "{command:?}");
        }

        assert!(!MessageCommand::Inv.is_single_queued());
        assert!(!MessageCommand::Unknown(0x99).is_single_queued());
    }

    #[test]
    fn outbound_queue_high_priority_set_matches_remote_node_policy() {
        assert_eq!(
            MessageCommand::HIGH_PRIORITY_COMMANDS,
            [
                MessageCommand::Alert,
                MessageCommand::Extensible,
                MessageCommand::FilterAdd,
                MessageCommand::FilterClear,
                MessageCommand::FilterLoad,
                MessageCommand::GetAddr,
                MessageCommand::Mempool,
            ]
        );

        for command in MessageCommand::HIGH_PRIORITY_COMMANDS {
            assert!(command.is_high_priority_queue(), "{command:?}");
        }

        assert!(!MessageCommand::Ping.is_high_priority_queue());
        assert!(!MessageCommand::Unknown(0x99).is_high_priority_queue());
    }

    #[test]
    fn outbound_class_batches_only_announcement_and_relay_commands() {
        assert_eq!(
            MessageCommand::BATCHABLE_COMMANDS,
            [
                MessageCommand::Inv,
                MessageCommand::Transaction,
                MessageCommand::Block,
                MessageCommand::MerkleBlock,
            ]
        );

        for command in MessageCommand::BATCHABLE_COMMANDS {
            assert_eq!(command.outbound_class(), OutboundClass::Batchable);
            assert!(command.is_batchable());
            assert!(!command.is_immediate());
        }
    }

    #[test]
    fn outbound_class_keeps_handshake_liveness_consensus_and_replies_immediate() {
        let immediate = [
            MessageCommand::Version,
            MessageCommand::Verack,
            MessageCommand::GetAddr,
            MessageCommand::Addr,
            MessageCommand::Ping,
            MessageCommand::Pong,
            MessageCommand::GetHeaders,
            MessageCommand::Headers,
            MessageCommand::GetBlocks,
            MessageCommand::Mempool,
            MessageCommand::GetData,
            MessageCommand::GetBlockByIndex,
            MessageCommand::NotFound,
            MessageCommand::Extensible,
            MessageCommand::Reject,
            MessageCommand::FilterLoad,
            MessageCommand::FilterAdd,
            MessageCommand::FilterClear,
            MessageCommand::Alert,
        ];

        for command in immediate {
            assert_eq!(command.outbound_class(), OutboundClass::Immediate);
            assert!(command.is_immediate());
            assert!(!command.is_batchable());
        }
    }

    #[test]
    fn every_protocol_command_is_classified_exactly_once() {
        let mut batchable = Vec::new();
        let mut immediate = 0usize;

        for byte in 0u8..=u8::MAX {
            let command = MessageCommand::from_byte(byte).expect("byte domain is infallible");
            if command.is_batchable() {
                batchable.push(command);
            } else {
                immediate += 1;
            }
        }

        assert_eq!(batchable, MessageCommand::BATCHABLE_COMMANDS.to_vec());
        assert_eq!(immediate, 252);
    }

    #[test]
    fn unknown_commands_stay_immediate() {
        assert_eq!(
            MessageCommand::Unknown(0x99).outbound_class(),
            OutboundClass::Immediate
        );
        assert!(!MessageCommand::Unknown(0x99).is_batchable());
    }
}
