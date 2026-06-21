//! Message command identifiers (mirrors `Neo.Network.P2P.MessageCommand`).

use neo_primitives::NetworkError;
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

    /// Returns true if this command should only have one item queued.
    pub fn is_single_queued(self) -> bool {
        Self::SINGLE_QUEUED_COMMANDS.contains(&self)
    }

    /// Returns true if this command should be processed with high priority.
    pub fn is_high_priority_queue(self) -> bool {
        Self::HIGH_PRIORITY_COMMANDS.contains(&self)
    }
}

#[cfg(test)]
#[path = "../tests/proto/message_command.rs"]
mod tests;
