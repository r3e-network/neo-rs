// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

use alloc::string::String;
use alloc::vec::Vec;

use bytes::{Buf, BufMut, Bytes, BytesMut};

pub trait BinWriter {
    // write varint in little endian
    fn write_varint(&mut self, value: u64);

    fn write<T: AsRef<[u8]>>(&mut self, value: T);

    fn len(&self) -> usize;
}

#[derive(Debug, PartialEq, Eq, Copy, Clone, thiserror::Error)]
pub enum BinDecodeError {
    #[error("bin-decode: end of the buffer")]
    EndOfBuffer,

    #[error("bin-decode: invalid value of '{0}' start at offset {1}")]
    InvalidValue(&'static str, usize),

    #[error("bin-decode: invalid type field '{2}' of '{0}' start at offset {1}")]
    InvalidType(&'static str, usize, u64),

    #[error("bin-decode: invalid length of '{1}' in '{0}'({2})")]
    InvalidLength(&'static str, &'static str, usize),
}

pub trait BinReader {
    fn read_varint_le(&mut self) -> Result<u64, BinDecodeError> {
        let mut buf = [0u8; 1];
        self.read_full(buf.as_mut_slice())?;
        match buf[0] {
            0xfd => {
                let mut buf = [0u8; 2];
                self.read_full(buf.as_mut_slice())?;
                Ok(u16::from_le_bytes(buf) as u64)
            }
            0xfe => {
                let mut buf = [0u8; 4];
                self.read_full(buf.as_mut_slice())?;
                Ok(u32::from_le_bytes(buf) as u64)
            }
            0xff => {
                let mut buf = [0u8; 8];
                self.read_full(buf.as_mut_slice())?;
                Ok(u64::from_le_bytes(buf))
            }
            n => Ok(n as u64), // just one byte
        }
    }

    /// like `read_exact`
    fn read_full(&mut self, buf: &mut [u8]) -> Result<(), BinDecodeError>;

    fn consumed(&self) -> usize;

    fn remaining(&self) -> usize;

    fn discard(&mut self, n: usize) -> usize;
}

pub struct BytesReader {
    consumed: usize,
    buf: Bytes,
    // max_bytes_size: usize, // TODO: add length limit
}

impl AsRef<[u8]> for BytesReader {
    fn as_ref(&self) -> &[u8] {
        self.buf.as_ref()
    }
}

impl From<Bytes> for BytesReader {
    fn from(value: Bytes) -> Self {
        Self {
            consumed: 0,
            buf: value,
        }
    }
}

impl From<BytesMut> for BytesReader {
    fn from(value: BytesMut) -> Self {
        Self {
            consumed: 0,
            buf: Bytes::from(value),
        }
    }
}

impl From<Vec<u8>> for BytesReader {
    fn from(value: Vec<u8>) -> Self {
        Self {
            consumed: 0,
            buf: Bytes::from(value),
        }
    }
}

impl BinWriter for BytesMut {
    fn write_varint(&mut self, value: u64) {
        let (size, buf) = to_varint_le(value);
        self.put_slice(&buf[..size as usize]); // size field
    }

    fn write<T: AsRef<[u8]>>(&mut self, value: T) {
        self.put_slice(value.as_ref());
    }

    fn len(&self) -> usize {
        self.len()
    }
}

impl BinReader for BytesReader {
    fn read_full(&mut self, buf: &mut [u8]) -> Result<(), BinDecodeError> {
        if self.remaining() < buf.len() {
            return Err(BinDecodeError::EndOfBuffer);
        }

        self.buf.copy_to_slice(buf);
        self.consumed += buf.len();
        Ok(())
    }

    fn consumed(&self) -> usize {
        self.consumed
    }

    fn remaining(&self) -> usize {
        self.buf.remaining()
    }

    fn discard(&mut self, n: usize) -> usize {
        let n = core::cmp::min(n, self.remaining());
        self.buf.advance(n);
        self.consumed += n;
        n
    }
}

pub trait BinEncoder {
    fn encode_bin(&self, w: &mut impl BinWriter);
}

pub trait BinDecoder: Sized {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError>;
}

impl BinEncoder for u8 {
    #[inline]
    fn encode_bin(&self, w: &mut impl BinWriter) {
        w.write(&self.to_le_bytes());
    }
}

impl BinEncoder for u16 {
    #[inline]
    fn encode_bin(&self, w: &mut impl BinWriter) {
        w.write(&self.to_le_bytes());
    }
}

impl BinEncoder for u32 {
    #[inline]
    fn encode_bin(&self, w: &mut impl BinWriter) {
        w.write(&self.to_le_bytes());
    }
}

impl BinEncoder for u64 {
    #[inline]
    fn encode_bin(&self, w: &mut impl BinWriter) {
        w.write(&self.to_le_bytes());
    }
}

impl BinEncoder for [u8] {
    #[inline]
    fn encode_bin(&self, w: &mut impl BinWriter) {
        w.write_varint(self.len() as u64);
        w.write(self);
    }
}

impl BinDecoder for bool {
    fn decode_bin(r: &mut impl BinReader) -> Result<bool, BinDecodeError> {
        let offset = r.consumed();
        let b = u8::decode_bin(r)?;
        if b != 0x0 && b != 0x1 {
            Err(BinDecodeError::InvalidValue("bool", offset))
        } else {
            Ok(b == 0x01)
        }
    }
}

impl BinDecoder for u8 {
    fn decode_bin(r: &mut impl BinReader) -> Result<u8, BinDecodeError> {
        let mut buf = [0u8; 1];
        r.read_full(buf.as_mut_slice())?;
        Ok(buf[0])
    }
}

impl BinDecoder for u16 {
    fn decode_bin(r: &mut impl BinReader) -> Result<u16, BinDecodeError> {
        let mut buf = [0u8; 2];
        r.read_full(buf.as_mut_slice())?;
        Ok(u16::from_le_bytes(buf))
    }
}

impl BinDecoder for u32 {
    fn decode_bin(r: &mut impl BinReader) -> Result<u32, BinDecodeError> {
        let mut buf = [0u8; 4];
        r.read_full(buf.as_mut_slice())?;
        Ok(u32::from_le_bytes(buf))
    }
}

impl BinDecoder for u64 {
    fn decode_bin(r: &mut impl BinReader) -> Result<u64, BinDecodeError> {
        let mut buf = [0u8; 8];
        r.read_full(buf.as_mut_slice())?;
        Ok(u64::from_le_bytes(buf))
    }
}

impl BinDecoder for String {
    fn decode_bin(r: &mut impl BinReader) -> Result<Self, BinDecodeError> {
        let offset = r.consumed();
        let len = r.read_varint_le()?;
        let mut buf = Vec::with_capacity(len as usize); // TODO: add length limit
        r.read_full(buf.as_mut_slice())?;
        String::from_utf8(buf).map_err(|_err| BinDecodeError::InvalidValue("String", offset))
    }
}

pub fn to_varint_le(value: u64) -> (u8, [u8; 9]) {
    let mut le = [0u8; 9];
    if value < 0xfd {
        le[0] = value as u8;
        (1, le)
    } else if value < 0xFFFF {
        le[0] = 0xfd;
        le[1..=2].copy_from_slice(&(value as u16).to_le_bytes());
        (3, le)
    } else if value < 0xFFFFFFFF {
        le[0] = 0xfe;
        le[1..=4].copy_from_slice(&(value as u32).to_le_bytes());
        (5, le)
    } else {
        le[0] = 0xff;
        le[1..=8].copy_from_slice(&value.to_le_bytes());
        (9, le)
    }
}
