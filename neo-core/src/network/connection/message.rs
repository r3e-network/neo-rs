use actix::prelude::*;
use serde::{Serialize, Deserialize};
use getset::{Getters, Setters};
use crate::io::binary_writer::BinaryWriter;
use crate::io::caching::{get_or_create_global_reflection_cache, ReflectionCache};
use crate::io::memory_reader::MemoryReader;
use crate::io::serializable_trait::SerializableTrait;
use crate::network::{MessageCommand, MessageFlags, RemoteNode};
use lz4::{block::compress as lz4_compress_block, block::decompress as lz4_decompress_block};
use crate::network::network_error::NetworkError;

#[derive(Getters, Setters, Clone)]
pub struct Message<T: SerializableTrait + Clone> {
    #[getset(get = "pub", set = "pub")]
    pub flags: MessageFlags,
    #[getset(get = "pub", set = "pub")]
    pub command: MessageCommand,
    #[getset(get = "pub", set = "pub")]
    pub payload: Option<T>,
    #[getset(get = "pub", set = "pub")]
    payload_compressed: Vec<u8>,
}

impl<T: SerializableTrait + Clone> Message<T> {
    pub const PAYLOAD_MAX_SIZE: usize = 0x02000000;
    const COMPRESSION_MIN_SIZE: usize = 128;
    const COMPRESSION_THRESHOLD: usize = 64;

    pub fn create(command: MessageCommand, payload: Option<T>) -> Self {
        let mut message = Message {
            flags: MessageFlags::None,
            command,
            payload: payload.clone(),
            payload_compressed: payload
                .as_ref()
                .map_or(Vec::new(), |p| p.to_vec()),
        };

        let try_compression = matches!(
            command,
            MessageCommand::Block
                | MessageCommand::Extensible
                | MessageCommand::Transaction
                | MessageCommand::Headers
                | MessageCommand::Addr
                | MessageCommand::MerkleBlock
                | MessageCommand::FilterLoad
                | MessageCommand::FilterAdd
        );

        if try_compression && message.payload_compressed.len() > Self::COMPRESSION_MIN_SIZE {
            let compressed = lz4_compress(&message.payload_compressed);
            if compressed.len()
                < message.payload_compressed.len() - Self::COMPRESSION_THRESHOLD
            {
                message.payload_compressed = compressed;
                message.flags = MessageFlags::Compressed;
            }
        }

        message
    }

    fn decompress_payload(&mut self) {
        if self.payload_compressed.is_empty() {
            return;
        }
        let decompressed = if self.flags == MessageFlags::Compressed {
            lz4_decompress(&self.payload_compressed, Self::PAYLOAD_MAX_SIZE)
        } else {
            self.payload_compressed.clone()
        };
        self.payload = Some(
            get_or_create_global_reflection_cache::<MessageCommand>()
                .lock()
                .unwrap()
                .create_serializable(self.command, &mut MemoryReader::new(&decompressed))
                .unwrap()
                .unwrap(),
        );
    }

    pub fn try_deserialize(data: &[u8]) -> Option<(Self, usize)> {
        if data.len() < 3 {
            return None;
        }

        let flags = MessageFlags::from_u8(data[0]).ok_or(||Err(NetworkError::Unknown("Unknown flag".to_string()))).ok()?;
        let command = MessageCommand::from(data[1]);
        let (length, payload_index) = match data[2] {
            0xFD => {
                if data.len() < 5 {
                    return None;
                }
                (u16::from_le_bytes([data[3], data[4]]) as usize, 5)
            }
            0xFE => {
                if data.len() < 7 {
                    return None;
                }
                (
                    u32::from_le_bytes([data[3], data[4], data[5], data[6]]) as usize,
                    7,
                )
            }
            0xFF => {
                if data.len() < 11 {
                    return None;
                }
                (
                    u64::from_le_bytes([
                        data[3], data[4], data[5], data[6], data[7], data[8], data[9],
                        data[10],
                    ]) as usize,
                    11,
                )
            }
            length => (length as usize, 3),
        };

        if length > Self::PAYLOAD_MAX_SIZE {
            return None;
        }

        if data.len() < length + payload_index {
            return None;
        }

        let mut msg = Message {
            flags,
            command,
            payload: None,
            payload_compressed: if length > 0 {
                data[payload_index..payload_index + length].to_vec()
            } else {
                Vec::new()
            },
        };
        msg.decompress_payload();

        Some((msg, payload_index + length))
    }
}

impl<T: SerializableTrait + Clone> SerializableTrait for Message<T> {
    fn size(&self) -> usize {
        std::mem::size_of::<MessageFlags>()
            + std::mem::size_of::<MessageCommand>()
            + self.payload_compressed.len()
    }

    fn serialize(&self, writer: &mut BinaryWriter) -> Result<(), std::io::Error> {
        writer.write_u8(self.flags.into())?;
        writer.write_u8(self.command.into())?;
        writer.write_var_bytes(&self.payload_compressed)?;
        Ok(())
    }

    fn deserialize(reader: &mut MemoryReader) -> Result<Self, std::io::Error> {
        let flags = MessageFlags::from_u8(reader.read_u8()?);
        let command = MessageCommand::from(reader.read_u8()?);
        let payload_compressed = reader.read_var_bytes(Self::PAYLOAD_MAX_SIZE)?;
        let mut msg = Message {
            flags,
            command,
            payload: None,
            payload_compressed,
        };
        msg.decompress_payload();
        Ok(msg)
    }
}

// Implement LZ4 compression using the `lz4` crate
fn lz4_compress(data: &[u8]) -> Vec<u8> {
    lz4_compress_block(data, None, false).unwrap_or_else(|_| data.to_vec())
}

// Implement LZ4 decompression using the `lz4` crate
fn lz4_decompress(data: &[u8], max_size: usize) -> Vec<u8> {
    lz4_decompress_block(data, Some(max_size as i32)).unwrap_or_else(|_| data.to_vec())
}


impl<T: SerializableTrait + Clone> actix::Message for Message<T> {
    type Result = ();
}

impl<T: SerializableTrait + Clone> Handler<Message<T>> for RemoteNode {
    type Result = ();

    fn handle(&mut self, msg: Message<T>, _ctx: &mut Self::Context) -> Self::Result {
        // Handle the message
        self.handle_message(msg);
    }
}