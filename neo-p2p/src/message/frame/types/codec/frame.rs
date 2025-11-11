use neo_base::encoding::{read_varint, write_varint, DecodeError, NeoRead, NeoWrite};

use crate::message::{
    command::MessageCommand,
    frame::{MessageFlags, COMPRESSION_MIN_SIZE, COMPRESSION_THRESHOLD, PAYLOAD_MAX_SIZE},
};

use super::super::{
    compression::{decompress_payload, try_compress},
    Message,
};
use super::{decode_payload, encode_payload};

pub(super) fn encode_inner(message: &Message, writer: &mut impl NeoWrite, allow_compression: bool) {
    let mut payload = Vec::new();
    encode_payload(message, &mut payload);

    let mut flags = MessageFlags::NONE;
    if allow_compression
        && message.command().allows_compression()
        && payload.len() >= COMPRESSION_MIN_SIZE
    {
        if let Ok(compressed) = try_compress(&payload) {
            if compressed.len() + COMPRESSION_THRESHOLD < payload.len() {
                payload = compressed;
                flags.insert(MessageFlags::COMPRESSED);
            }
        }
    }

    writer.write_u8(flags.bits());
    writer.write_u8(message.command() as u8);
    write_varint(writer, payload.len() as u64);
    writer.write_bytes(&payload);
}

pub(super) fn decode_inner(reader: &mut impl NeoRead) -> Result<Message, DecodeError> {
    let flags = MessageFlags::from_bits(reader.read_u8()?)?;
    let command = MessageCommand::try_from(reader.read_u8()?)?;
    let payload_len = read_varint(reader)? as usize;
    if payload_len > PAYLOAD_MAX_SIZE {
        return Err(DecodeError::LengthOutOfRange {
            len: payload_len as u64,
            max: PAYLOAD_MAX_SIZE as u64,
        });
    }

    let mut payload = vec![0u8; payload_len];
    reader.read_into(&mut payload)?;
    let payload = if flags.contains(MessageFlags::COMPRESSED) {
        decompress_payload(&payload)?
    } else {
        payload
    };
    decode_payload(command, &payload)
}
