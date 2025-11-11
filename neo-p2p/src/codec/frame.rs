use std::io;

use bytes::{Buf, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use neo_base::encoding::{read_varint, write_varint, DecodeError, NeoDecode, SliceReader};

use crate::message::{Message, PAYLOAD_MAX_SIZE};

#[derive(Default)]
pub struct NeoMessageCodec {
    expected_len: Option<usize>,
    compression_allowed: bool,
}

impl NeoMessageCodec {
    pub fn new() -> Self {
        Self {
            expected_len: None,
            compression_allowed: false,
        }
    }

    pub fn with_compression_allowed(mut self, allowed: bool) -> Self {
        self.compression_allowed = allowed;
        self
    }

    pub fn set_compression_allowed(&mut self, allowed: bool) {
        self.compression_allowed = allowed;
    }

    fn read_length(src: &mut BytesMut) -> io::Result<Option<usize>> {
        if src.is_empty() {
            return Ok(None);
        }

        let mut reader = SliceReader::new(src.as_ref());
        match read_varint(&mut reader) {
            Ok(len) => {
                let consumed = reader.consumed();
                src.advance(consumed);
                Ok(Some(len as usize))
            }
            Err(DecodeError::UnexpectedEof { .. }) => Ok(None),
            Err(err) => Err(io::Error::new(io::ErrorKind::InvalidData, err)),
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

        if self.compression_allowed {
            // compression handled inside message encoding flags; no-op for now
        }

        write_varint(dst, payload.len() as u64);
        dst.extend_from_slice(&payload);
        Ok(())
    }
}

impl Decoder for NeoMessageCodec {
    type Item = Message;
    type Error = io::Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let len = match self.expected_len {
            Some(len) => len,
            None => match Self::read_length(src)? {
                Some(len) => {
                    if len > PAYLOAD_MAX_SIZE {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "message too large",
                        ));
                    }
                    self.expected_len = Some(len);
                    len
                }
                None => return Ok(None),
            },
        };

        if src.len() < len {
            return Ok(None);
        }

        let payload = src.split_to(len);
        self.expected_len = None;

        let mut reader = SliceReader::new(payload.as_ref());
        Message::neo_decode(&mut reader)
            .map(Some)
            .map_err(|err| io::Error::new(io::ErrorKind::InvalidData, err))
    }
}
