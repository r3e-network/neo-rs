mod frame;
mod payload;

use frame::{decode_inner as frame_decode, encode_inner as frame_encode};
use payload::{decode_payload as payload_decode, encode_payload as payload_encode};

pub(super) fn encode_inner(message: &Message, writer: &mut impl NeoWrite, allow_compression: bool) {
    frame_encode(message, writer, allow_compression)
}

pub(super) fn decode_inner(reader: &mut impl NeoRead) -> Result<Message, DecodeError> {
    frame_decode(reader)
}

pub(super) fn encode_payload(message: &Message, buf: &mut Vec<u8>) {
    payload_encode(message, buf)
}

pub(super) fn decode_payload(
    command: MessageCommand,
    payload: &[u8],
) -> Result<Message, DecodeError> {
    payload_decode(command, payload)
}
use std::vec::Vec;

use neo_base::{encoding::DecodeError, NeoRead, NeoWrite};

use crate::message::command::MessageCommand;

use super::Message;
