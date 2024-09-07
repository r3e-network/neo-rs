// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use std::io::{Error as IoError, ErrorKind::InvalidData};

use tokio_util::bytes::{Buf, BufMut, BytesMut};
use tokio_util::codec::{Decoder, Encoder};

use neo_base::encoding::bin::*;
use neo_core::payload::{self, P2pMessage, Lz4Compress, Lz4Decompress};
use neo_core::types::Bytes;


const MAX_BODY_LEN: usize = 0x02000000; // 32MiB

const FLAG_COMPRESSED: u8 = 0x01;


pub trait MessageWrite {
    fn write_message(&mut self, flags: u8, cmd: u8, data: &[u8]) -> Result<(), IoError>;
}

impl MessageWrite for BytesMut {
    #[inline]
    fn write_message(&mut self, flags: u8, cmd: u8, data: &[u8]) -> Result<(), IoError> {
        if data.len() > MAX_BODY_LEN {
            return Err(IoError::new(InvalidData, format!("encoder: too large body {}-{}-{}", flags, cmd, data.len())));
        }

        self.reserve(1 + 2 + 5 + data.len());
        self.put_u8(flags); // no-compress
        self.put_u8(cmd);
        self.write_varint_le(data.len() as u64);
        self.write(data);
        Ok(())
    }
}

pub struct MessageEncoder;

impl Encoder<Bytes> for MessageEncoder {
    type Error = IoError;

    // `item` should be serialized(encode_bin) from P2pMessage
    fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> Result<(), Self::Error> {
        if item.len() == 0 {
            return Err(IoError::new(InvalidData, "encoder: unexpected empty data"));
        }

        let data = item.as_bytes();
        if payload::can_compress(data[0]) {
            if let Ok(compressed) = (&data[1..]).lz4_compress() {
                dst.write_message(FLAG_COMPRESSED, data[0], &compressed)?;
            } else {
                dst.write_message(0, data[0], &data[1..])?;
            }
        } else {
            dst.write_message(0, data[0], &data[1..])?;
        }

        Ok(())
    }
}

pub trait ToMessageEncoded {
    type Error;

    fn to_message_encoded(&self) -> Result<BytesMut, Self::Error>;
}

impl ToMessageEncoded for P2pMessage {
    type Error = IoError;

    #[inline]
    fn to_message_encoded(&self) -> Result<BytesMut, Self::Error> {
        let mut encoder = MessageEncoder;
        let mut buf = BytesMut::with_capacity(self.bin_size());

        encoder.encode(self.to_bin_encoded().into(), &mut buf)?;
        Ok(buf)
    }
}


pub struct MessageDecoder;

impl Decoder for MessageDecoder {
    type Item = Bytes;
    type Error = IoError;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.len() < 3 {
            return Ok(None);
        }

        let d = src.as_ref();
        let prefix = d[2];
        let (head, body) = match prefix {
            0x00..=0xfc => (3, prefix as u64),
            0xfd if d.len() >= 5 => (5, u16::from_le_bytes([d[3], d[4]]) as u64),
            0xfe if d.len() >= 7 => (7, u32::from_le_bytes([d[3], d[4], d[5], d[6]]) as u64),
            0xff if d.len() >= 11 => (11, u64::from_le_bytes([d[3], d[4], d[5], d[6], d[7], d[8], d[9], d[10]])),
            _ => return Err(IoError::new(InvalidData, format!("decoder: invalid prefix 0x{:x}", prefix))),
        };

        let body = body as usize;
        if body > MAX_BODY_LEN {
            return Err(IoError::new(InvalidData, format!("decoder: too large body {}-{}-{}", d[0], d[1], body)));
        }

        let total = head + body;
        if src.len() < total {
            return Ok(None);
        }

        let build = |data: &[u8]| {
            let mut frame = Vec::with_capacity(1 + data.len());
            frame.push(src[1]);
            frame.extend_from_slice(&data);
            frame
        };

        let body = if src[0] & FLAG_COMPRESSED != 0 {
            let body: Vec<u8> = Lz4Decompress::lz4_decompress(&src[head..total])
                .map_err(|err| IoError::new(InvalidData, format!("decoder: lz4_decompress {}", err)))?;
            build(&body)
        } else {
            build(&src[head..total])
        };

        src.advance(total);
        Ok(Some(body.into()))
    }
}
