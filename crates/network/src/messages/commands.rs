//! Neo 3 Message command definitions.
//!
//! This module provides the correct Neo 3 message command format using single-byte enum values
//! as implemented in the C# Neo source code.

use serde::{Deserialize, Serialize};

/// Neo 3 message command (single byte enum, not 12-byte string)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum MessageCommand {
    /// Version message (0x00)
    Version = 0x00,
    /// Version acknowledgment (0x01)  
    Verack = 0x01,
    /// Get addresses (0x10)
    GetAddr = 0x10,
    /// Addresses (0x11)
    Addr = 0x11,
    /// Ping (0x18)
    Ping = 0x18,
    /// Pong (0x19)
    Pong = 0x19,
    /// Get headers (0x20)
    GetHeaders = 0x20,
    /// Headers (0x21)
    Headers = 0x21,
    /// Get blocks (0x24)
    GetBlocks = 0x24,
    /// Mempool (0x25)
    Mempool = 0x25,
    /// Inventory (0x27)
    Inv = 0x27,
    /// Get data (0x28)
    GetData = 0x28,
    /// Get block by index (0x29)
    GetBlockByIndex = 0x29,
    /// Not found (0x2a)
    NotFound = 0x2a,
    /// Transaction (0x2b)
    Transaction = 0x2b,
    /// Block (0x2c)
    Block = 0x2c,
    /// Extensible (0x2e)
    Extensible = 0x2e,
    /// Reject (0x2f)
    Reject = 0x2f,
    /// Filter load (0x30)
    FilterLoad = 0x30,
    /// Filter add (0x31)
    FilterAdd = 0x31,
    /// Filter clear (0x32)
    FilterClear = 0x32,
    /// Merkle block (0x38)
    MerkleBlock = 0x38,
    /// Alert (0x40)
    Alert = 0x40,
    /// Unknown/Undocumented command (0xbe) - seen in some peer implementations
    Unknown = 0xbe,
    /// Version with payload (0x55) - peer version with user agent
    VersionWithPayload = 0x55,
}

impl MessageCommand {
    /// Gets the byte value of the command
    pub fn as_byte(self) -> u8 {
        self as u8
    }

    /// Creates a command from byte value
    pub fn from_byte(byte: u8) -> Result<Self, crate::NetworkError> {
        match byte {
            0x00 => Ok(Self::Version),
            0x01 => Ok(Self::Verack),
            0x10 => Ok(Self::GetAddr),
            0x11 => Ok(Self::Addr),
            0x18 => Ok(Self::Ping),
            0x19 => Ok(Self::Pong),
            0x20 => Ok(Self::GetHeaders),
            0x21 => Ok(Self::Headers),
            0x24 => Ok(Self::GetBlocks),
            0x25 => Ok(Self::Mempool),
            0x27 => Ok(Self::Inv),
            0x28 => Ok(Self::GetData),
            0x29 => Ok(Self::GetBlockByIndex),
            0x2a => Ok(Self::NotFound),
            0x2b => Ok(Self::Transaction),
            0x2c => Ok(Self::Block),
            0x2e => Ok(Self::Extensible),
            0x2f => Ok(Self::Reject),
            0x30 => Ok(Self::FilterLoad),
            0x31 => Ok(Self::FilterAdd),
            0x32 => Ok(Self::FilterClear),
            0x38 => Ok(Self::MerkleBlock),
            0x40 => Ok(Self::Alert),
            0x55 => Ok(Self::VersionWithPayload),
            0xbe => Ok(Self::Unknown),
            _ => Err(crate::NetworkError::ProtocolViolation {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                violation: format!("Unknown command byte: 0x{:02x}", byte),
            }),
        }
    }

    /// Creates a command from string representation (for backwards compatibility)
    pub fn from_str(s: &str) -> Result<Self, crate::NetworkError> {
        match s {
            "version" => Ok(Self::Version),
            "verack" => Ok(Self::Verack),
            "getaddr" => Ok(Self::GetAddr),
            "addr" => Ok(Self::Addr),
            "ping" => Ok(Self::Ping),
            "pong" => Ok(Self::Pong),
            "getheaders" => Ok(Self::GetHeaders),
            "headers" => Ok(Self::Headers),
            "getblocks" => Ok(Self::GetBlocks),
            "mempool" => Ok(Self::Mempool),
            "inv" => Ok(Self::Inv),
            "getdata" => Ok(Self::GetData),
            "getblkbyidx" => Ok(Self::GetBlockByIndex),
            "tx" => Ok(Self::Transaction),
            "block" => Ok(Self::Block),
            "notfound" => Ok(Self::NotFound),
            "reject" => Ok(Self::Reject),
            "extensible" => Ok(Self::Extensible),
            "filterload" => Ok(Self::FilterLoad),
            "filteradd" => Ok(Self::FilterAdd),
            "filterclear" => Ok(Self::FilterClear),
            "merkleblock" => Ok(Self::MerkleBlock),
            "alert" => Ok(Self::Alert),
            "versionwithpayload" => Ok(Self::VersionWithPayload),
            "unknown" => Ok(Self::Unknown),
            _ => Err(crate::NetworkError::ProtocolViolation {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                violation: format!("Unknown command: {}", s),
            }),
        }
    }

    /// Gets the string name of the command
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Version => "version",
            Self::Verack => "verack",
            Self::GetAddr => "getaddr",
            Self::Addr => "addr",
            Self::Ping => "ping",
            Self::Pong => "pong",
            Self::GetHeaders => "getheaders",
            Self::Headers => "headers",
            Self::GetBlocks => "getblocks",
            Self::Mempool => "mempool",
            Self::Inv => "inv",
            Self::GetData => "getdata",
            Self::GetBlockByIndex => "getblkbyidx",
            Self::NotFound => "notfound",
            Self::Transaction => "tx",
            Self::Block => "block",
            Self::Extensible => "extensible",
            Self::Reject => "reject",
            Self::FilterLoad => "filterload",
            Self::FilterAdd => "filteradd",
            Self::FilterClear => "filterclear",
            Self::MerkleBlock => "merkleblock",
            Self::Alert => "alert",
            Self::VersionWithPayload => "versionwithpayload",
            Self::Unknown => "unknown",
        }
    }
}

impl std::fmt::Display for MessageCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Neo 3 message flags (single byte)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[repr(u8)]
pub enum MessageFlags {
    /// No flags (0x00)
    None = 0x00,
    /// Compressed payload (0x01)
    Compressed = 0x01,
}

impl MessageFlags {
    /// Gets the byte value of the flags
    pub fn as_byte(self) -> u8 {
        self as u8
    }

    /// Creates flags from byte value
    pub fn from_byte(byte: u8) -> Result<Self, crate::NetworkError> {
        match byte {
            0x00 => Ok(Self::None),
            0x01 => Ok(Self::Compressed),
            _ => Err(crate::NetworkError::ProtocolViolation {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                violation: format!("Unknown flags byte: 0x{:02x}", byte),
            }),
        }
    }

    /// Checks if compression is enabled
    pub fn is_compressed(self) -> bool {
        matches!(self, Self::Compressed)
    }
}

/// Helper functions for variable-length encoding used in Neo 3
pub mod varlen {
    use crate::NetworkError;

    /// Encodes a length value using Neo 3 variable-length encoding
    pub fn encode_length(len: usize) -> Vec<u8> {
        if len < 0xfd {
            vec![len as u8]
        } else if len <= 0xffff {
            let mut bytes = vec![0xfd];
            bytes.extend_from_slice(&(len as u16).to_le_bytes());
            bytes
        } else if len <= 0xffffffff {
            let mut bytes = vec![0xfe];
            bytes.extend_from_slice(&(len as u32).to_le_bytes());
            bytes
        } else {
            let mut bytes = vec![0xff];
            bytes.extend_from_slice(&(len as u64).to_le_bytes());
            bytes
        }
    }

    /// Decodes a length value from Neo 3 variable-length encoding
    pub fn decode_length(bytes: &[u8]) -> Result<(usize, usize), NetworkError> {
        if bytes.is_empty() {
            return Err(NetworkError::ProtocolViolation {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                violation: "Empty length data".to_string(),
            });
        }

        match bytes[0] {
            len @ 0..=252 => Ok((len as usize, 1)),
            0xfd => {
                if bytes.len() < 3 {
                    return Err(NetworkError::ProtocolViolation {
                        peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                        violation: "Insufficient data for 2-byte length".to_string(),
                    });
                }
                let len = u16::from_le_bytes([bytes[1], bytes[2]]) as usize;
                Ok((len, 3))
            }
            0xfe => {
                if bytes.len() < 5 {
                    return Err(NetworkError::ProtocolViolation {
                        peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                        violation: "Insufficient data for 4-byte length".to_string(),
                    });
                }
                let len = u32::from_le_bytes([bytes[1], bytes[2], bytes[3], bytes[4]]) as usize;
                Ok((len, 5))
            }
            0xff => {
                if bytes.len() < 9 {
                    return Err(NetworkError::ProtocolViolation {
                        peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                        violation: "Insufficient data for 8-byte length".to_string(),
                    });
                }
                let len = u64::from_le_bytes([
                    bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7], bytes[8],
                ]) as usize;
                Ok((len, 9))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_command() {
        let version_cmd = MessageCommand::Version;
        assert_eq!(version_cmd.to_string(), "version");
        assert_eq!(version_cmd.as_byte(), 0x00);

        let ping_cmd = MessageCommand::Ping;
        assert_eq!(ping_cmd.to_string(), "ping");
        assert_eq!(ping_cmd.as_byte(), 0x18);
    }

    #[test]
    fn test_command_from_byte() {
        assert_eq!(
            MessageCommand::from_byte(0x00).unwrap(),
            MessageCommand::Version
        );
        assert_eq!(
            MessageCommand::from_byte(0x01).unwrap(),
            MessageCommand::Verack
        );
        assert_eq!(
            MessageCommand::from_byte(0x18).unwrap(),
            MessageCommand::Ping
        );

        // Test invalid command
        assert!(MessageCommand::from_byte(0xff).is_err());
    }

    #[test]
    fn test_message_flags() {
        let none_flags = MessageFlags::None;
        assert_eq!(none_flags.as_byte(), 0x00);
        assert!(!none_flags.is_compressed());

        let compressed_flags = MessageFlags::Compressed;
        assert_eq!(compressed_flags.as_byte(), 0x01);
        assert!(compressed_flags.is_compressed());
    }

    #[test]
    fn test_varlen_encoding() {
        use varlen::*;

        // Test small length (< 253)
        assert_eq!(encode_length(100), vec![100]);

        // Test medium length (253-65535)
        assert_eq!(encode_length(1000), vec![0xfd, 0xe8, 0x03]);

        // Test length decoding
        let (len, consumed) = decode_length(&[100]).unwrap();
        assert_eq!(len, 100);
        assert_eq!(consumed, 1);

        let (len, consumed) = decode_length(&[0xfd, 0xe8, 0x03]).unwrap();
        assert_eq!(len, 1000);
        assert_eq!(consumed, 3);
    }
}
