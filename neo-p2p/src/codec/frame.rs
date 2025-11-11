use std::io;

use bytes::BytesMut;
use tokio_util::codec::{Decoder, Encoder};

use neo_base::{
    encoding::{NeoDecode, SliceReader},
    hash::double_sha256,
};

use crate::message::{Message, MessageCommand, PAYLOAD_MAX_SIZE};

const COMMAND_NAME_LEN: usize = 12;
const HEADER_LEN: usize = 4 + COMMAND_NAME_LEN + 4 + 4;

#[derive(Default)]
pub struct NeoMessageCodec {
    compression_allowed: bool,
    network_magic: u32,
}

impl NeoMessageCodec {
    pub fn new() -> Self {
        Self {
            compression_allowed: false,
            network_magic: 0,
        }
    }

    pub fn with_compression_allowed(mut self, allowed: bool) -> Self {
        self.compression_allowed = allowed;
        self
    }

    pub fn with_network_magic(mut self, magic: u32) -> Self {
        self.network_magic = magic;
        self
    }

    pub fn set_compression_allowed(&mut self, allowed: bool) {
        self.compression_allowed = allowed;
    }

    pub fn set_network_magic(&mut self, magic: u32) {
        self.network_magic = magic;
    }

    fn network_magic(&self) -> io::Result<u32> {
        if self.network_magic == 0 {
            Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "network magic not configured",
            ))
        } else {
            Ok(self.network_magic)
        }
    }
}

impl Encoder<Message> for NeoMessageCodec {
    type Error = io::Error;

    fn encode(&mut self, item: Message, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let mut payload = Vec::new();
        item.neo_encode_with_compression(&mut payload, self.compression_allowed);
        if payload.len() > PAYLOAD_MAX_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "message too large",
            ));
        }

        let magic = self.network_magic()?;
        let mut header = [0u8; HEADER_LEN];
        header[..4].copy_from_slice(&magic.to_le_bytes());
        let name_bytes = command_name_bytes(item.command());
        header[4..4 + COMMAND_NAME_LEN].copy_from_slice(&name_bytes);
        header[16..20].copy_from_slice(&(payload.len() as u32).to_le_bytes());
        let checksum = double_sha256(&payload);
        header[20..24].copy_from_slice(&checksum[..4]);

        dst.extend_from_slice(&header);
        dst.extend_from_slice(&payload);
        Ok(())
    }
}

impl Decoder for NeoMessageCodec {
    type Item = Message;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < HEADER_LEN {
            return Ok(None);
        }
        let mut header = [0u8; HEADER_LEN];
        header.copy_from_slice(&src[..HEADER_LEN]);
        let magic = u32::from_le_bytes(header[0..4].try_into().unwrap());
        let expected_magic = self.network_magic()?;
        if magic != expected_magic {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("network magic mismatch (expected {expected_magic:#x}, got {magic:#x})"),
            ));
        }
        let mut name_buf = [0u8; COMMAND_NAME_LEN];
        name_buf.copy_from_slice(&header[4..16]);
        let header_command = parse_command_name(&name_buf)?;
        let payload_len = u32::from_le_bytes(header[16..20].try_into().unwrap()) as usize;
        if payload_len > PAYLOAD_MAX_SIZE {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "message too large",
            ));
        }
        if src.len() < HEADER_LEN + payload_len {
            return Ok(None);
        }

        let payload = src.split_to(HEADER_LEN + payload_len).split_off(HEADER_LEN);
        let checksum = &header[20..24];
        let computed = double_sha256(&payload);
        if computed[..4] != checksum[..] {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "checksum mismatch",
            ));
        }

        let mut reader = SliceReader::new(payload.as_ref());

        let message = Message::neo_decode(&mut reader)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))?;
        if message.command() != header_command {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "header command mismatch",
            ));
        }
        Ok(Some(message))
    }
}

fn command_name_bytes(command: MessageCommand) -> [u8; COMMAND_NAME_LEN] {
    let mut buf = [0u8; COMMAND_NAME_LEN];
    let name = command.as_str().as_bytes();
    debug_assert!(
        name.len() <= COMMAND_NAME_LEN,
        "command name exceeds {} bytes",
        COMMAND_NAME_LEN
    );
    buf[..name.len()].copy_from_slice(name);
    buf
}

fn parse_command_name(bytes: &[u8; COMMAND_NAME_LEN]) -> io::Result<MessageCommand> {
    let end = bytes
        .iter()
        .position(|&b| b == 0)
        .unwrap_or(COMMAND_NAME_LEN);
    let slice = &bytes[..end];
    let name = core::str::from_utf8(slice)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid command name"))?;
    MessageCommand::from_name(name)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "unknown command name"))
}
