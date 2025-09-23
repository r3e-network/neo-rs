//! Legacy compatibility layer exporting message command utilities.

pub use super::message_command::MessageCommand;
pub use super::message_flags::MessageFlags;

/// Helper functions for variable-length encoding used in Neo 3
pub mod varlen {
    use crate::NetworkError;

    /// Encodes a length value using Neo variable-length encoding.
    pub fn encode_length(len: usize) -> Vec<u8> {
        if len <= 0xfc {
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

    /// Decodes a length value from Neo variable-length encoding.
    pub fn decode_length(bytes: &[u8]) -> Result<(usize, usize), NetworkError> {
        if bytes.is_empty() {
            return Err(NetworkError::ProtocolViolation {
                peer: std::net::SocketAddr::from(([0, 0, 0, 0], 0)),
                violation: "Empty length data".to_string(),
            });
        }

        match bytes[0] {
            value @ 0..=252 => Ok((value as usize, 1)),
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
    use super::{varlen, MessageCommand, MessageFlags};

    #[test]
    fn message_command_roundtrip() {
        let version = MessageCommand::Version;
        assert_eq!(version.to_byte(), 0x00);
        assert_eq!(version.as_str(), "version");

        let ping = MessageCommand::from_byte(0x18).unwrap();
        assert_eq!(ping, MessageCommand::Ping);
        assert_eq!(ping.to_string(), "ping");
    }

    #[test]
    fn message_command_unknown() {
        let cmd = MessageCommand::from_byte(0xff).unwrap();
        assert_eq!(cmd, MessageCommand::Unknown(0xff));
        assert_eq!(cmd.to_byte(), 0xff);
        assert_eq!(cmd.as_str(), "unknown");
    }

    #[test]
    fn message_flags_roundtrip() {
        let flags = MessageFlags::Compressed;
        assert_eq!(flags.to_byte(), 0x01);
        assert!(flags.is_compressed());

        let parsed = MessageFlags::from_byte(0x00).unwrap();
        assert_eq!(parsed, MessageFlags::None);
        assert!(!parsed.is_compressed());
    }

    #[test]
    fn varlen_encoding() {
        assert_eq!(varlen::encode_length(100), vec![100]);
        assert_eq!(varlen::encode_length(1000), vec![0xfd, 0xe8, 0x03]);

        let (len, consumed) = varlen::decode_length(&[200]).unwrap();
        assert_eq!(len, 200);
        assert_eq!(consumed, 1);

        let (len, consumed) = varlen::decode_length(&[0xfe, 0x00, 0x10, 0x00, 0x00]).unwrap();
        assert_eq!(len, 4096);
        assert_eq!(consumed, 5);
    }
}
