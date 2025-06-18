//! Message command definitions.
//!
//! This module provides message command types exactly matching C# Neo MessageCommand.

use serde::{Deserialize, Serialize};

/// Network message command (12 bytes, zero-padded)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageCommand([u8; 12]);

impl MessageCommand {
    /// Creates a command from string
    pub fn new(cmd: &str) -> Self {
        let mut bytes = [0u8; 12];
        let cmd_bytes = cmd.as_bytes();
        let len = std::cmp::min(cmd_bytes.len(), 12);
        bytes[..len].copy_from_slice(&cmd_bytes[..len]);
        Self(bytes)
    }

    /// Version command
    pub const VERSION: MessageCommand = MessageCommand(*b"version\0\0\0\0\0");
    /// Version acknowledgment command
    pub const VERACK: MessageCommand = MessageCommand(*b"verack\0\0\0\0\0\0");
    /// Get addresses command
    pub const GETADDR: MessageCommand = MessageCommand(*b"getaddr\0\0\0\0\0");
    /// Addresses command
    pub const ADDR: MessageCommand = MessageCommand(*b"addr\0\0\0\0\0\0\0\0");
    /// Ping command
    pub const PING: MessageCommand = MessageCommand(*b"ping\0\0\0\0\0\0\0\0");
    /// Pong command
    pub const PONG: MessageCommand = MessageCommand(*b"pong\0\0\0\0\0\0\0\0");
    /// Get headers command
    pub const GETHEADERS: MessageCommand = MessageCommand(*b"getheaders\0\0");
    /// Headers command
    pub const HEADERS: MessageCommand = MessageCommand(*b"headers\0\0\0\0\0");
    /// Get blocks command
    pub const GETBLOCKS: MessageCommand = MessageCommand(*b"getblocks\0\0\0");
    /// Mempool command
    pub const MEMPOOL: MessageCommand = MessageCommand(*b"mempool\0\0\0\0\0");
    /// Inventory command
    pub const INV: MessageCommand = MessageCommand(*b"inv\0\0\0\0\0\0\0\0\0");
    /// Get data command
    pub const GETDATA: MessageCommand = MessageCommand(*b"getdata\0\0\0\0\0");
    /// Get block by index command
    pub const GETBLOCKS_BY_INDEX: MessageCommand = MessageCommand(*b"getblkbyidx\0");
    /// Transaction command
    pub const TX: MessageCommand = MessageCommand(*b"tx\0\0\0\0\0\0\0\0\0\0");
    /// Block command
    pub const BLOCK: MessageCommand = MessageCommand(*b"block\0\0\0\0\0\0\0");
    /// Not found command
    pub const NOTFOUND: MessageCommand = MessageCommand(*b"notfound\0\0\0\0");
    /// Reject command
    pub const REJECT: MessageCommand = MessageCommand(*b"reject\0\0\0\0\0\0");
    /// Filter load command
    pub const FILTERLOAD: MessageCommand = MessageCommand(*b"filterload\0\0");
    /// Filter add command
    pub const FILTERADD: MessageCommand = MessageCommand(*b"filteradd\0\0\0");
    /// Filter clear command
    pub const FILTERCLEAR: MessageCommand = MessageCommand(*b"filterclear\0");
    /// Merkle block command
    pub const MERKLEBLOCK: MessageCommand = MessageCommand(*b"merkleblock\0");
    /// Alert command
    pub const ALERT: MessageCommand = MessageCommand(*b"alert\0\0\0\0\0\0\0");

    /// Gets the raw bytes of the command
    pub fn as_bytes(&self) -> &[u8; 12] {
        &self.0
    }
}

impl std::fmt::Display for MessageCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let cmd_str = std::str::from_utf8(&self.0)
            .unwrap_or("invalid")
            .trim_end_matches('\0');
        write!(f, "{}", cmd_str)
    }
}

/// Network message types (for compatibility)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum MessageType {
    Version,
    Verack,
    GetAddr,
    Addr,
    Ping,
    Pong,
    GetHeaders,
    Headers,
    GetBlocks,
    Mempool,
    Inv,
    GetData,
    GetBlockByIndex,
    Tx,
    Block,
    NotFound,
    Reject,
    FilterLoad,
    FilterAdd,
    FilterClear,
    MerkleBlock,
    Alert,
}

impl MessageType {
    /// Gets the command for this message type
    pub fn command(&self) -> MessageCommand {
        match self {
            MessageType::Version => MessageCommand::VERSION,
            MessageType::Verack => MessageCommand::VERACK,
            MessageType::GetAddr => MessageCommand::GETADDR,
            MessageType::Addr => MessageCommand::ADDR,
            MessageType::Ping => MessageCommand::PING,
            MessageType::Pong => MessageCommand::PONG,
            MessageType::GetHeaders => MessageCommand::GETHEADERS,
            MessageType::Headers => MessageCommand::HEADERS,
            MessageType::GetBlocks => MessageCommand::GETBLOCKS,
            MessageType::Mempool => MessageCommand::MEMPOOL,
            MessageType::Inv => MessageCommand::INV,
            MessageType::GetData => MessageCommand::GETDATA,
            MessageType::GetBlockByIndex => MessageCommand::GETBLOCKS_BY_INDEX,
            MessageType::Tx => MessageCommand::TX,
            MessageType::Block => MessageCommand::BLOCK,
            MessageType::NotFound => MessageCommand::NOTFOUND,
            MessageType::Reject => MessageCommand::REJECT,
            MessageType::FilterLoad => MessageCommand::FILTERLOAD,
            MessageType::FilterAdd => MessageCommand::FILTERADD,
            MessageType::FilterClear => MessageCommand::FILTERCLEAR,
            MessageType::MerkleBlock => MessageCommand::MERKLEBLOCK,
            MessageType::Alert => MessageCommand::ALERT,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_command() {
        let version_cmd = MessageCommand::VERSION;
        assert_eq!(version_cmd.to_string(), "version");
        
        let custom_cmd = MessageCommand::new("test");
        assert_eq!(custom_cmd.to_string(), "test");
        
        // Test that commands are properly zero-padded
        assert_eq!(version_cmd.as_bytes().len(), 12);
    }

    #[test]
    fn test_message_type_command_mapping() {
        assert_eq!(MessageType::Version.command(), MessageCommand::VERSION);
        assert_eq!(MessageType::Ping.command(), MessageCommand::PING);
        assert_eq!(MessageType::Block.command(), MessageCommand::BLOCK);
    }
} 