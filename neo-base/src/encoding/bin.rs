use alloc::{string::String, vec, vec::Vec};

use bytes::{Buf, BufMut, Bytes, BytesMut};

/// Error returned when a value cannot be decoded from the Neo binary wire format.
#[derive(Debug, Copy, Clone, PartialEq, Eq, thiserror::Error)]
pub enum DecodeError {
    #[error("neo-bin: unexpected end of input (needed {needed}, remaining {remaining})")]
    UnexpectedEof { needed: usize, remaining: usize },

    #[error("neo-bin: invalid varint prefix 0x{0:02X}")]
    InvalidVarIntTag(u8),

    #[error("neo-bin: encoded length {len} exceeds maximum {max}")]
    LengthOutOfRange { len: u64, max: u64 },

    #[error("neo-bin: invalid utf-8 in string field")]
    InvalidUtf8,

    #[error("neo-bin: invalid value for {0}")]
    InvalidValue(&'static str),
}

/// All values that can be encoded into the Neo binary wire format implement this trait.
pub trait NeoEncode {
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W);

    #[inline]
    fn to_vec(&self) -> Vec<u8> {
        let mut buf = Vec::new();
        self.neo_encode(&mut buf);
        buf
    }
}

/// Values that can be decoded from the Neo binary wire format implement this trait.
pub trait NeoDecode: Sized {
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError>;
}

/// Writer abstraction that matches the Neo binary format.
pub trait NeoWrite {
    /// Append raw bytes to the destination buffer.
    fn write_bytes(&mut self, bytes: &[u8]);

    /// Number of bytes written so far.
    fn bytes_written(&self) -> usize;

    #[inline]
    fn write_u8(&mut self, value: u8) {
        self.write_bytes(&value.to_le_bytes());
    }

    #[inline]
    fn write_u16(&mut self, value: u16) {
        self.write_bytes(&value.to_le_bytes());
    }

    #[inline]
    fn write_u32(&mut self, value: u32) {
        self.write_bytes(&value.to_le_bytes());
    }

    #[inline]
    fn write_u64(&mut self, value: u64) {
        self.write_bytes(&value.to_le_bytes());
    }

    #[inline]
    fn write_var_bytes(&mut self, value: &[u8])
    where
        Self: Sized,
    {
        write_varint(self, value.len() as u64);
        self.write_bytes(value);
    }
}

/// Reader abstraction for the Neo binary format.
pub trait NeoRead {
    /// Attempt to read exactly `buf.len()` bytes into the provided slice.
    fn read_into(&mut self, buf: &mut [u8]) -> Result<(), DecodeError>;

    /// Remaining bytes that can be read from this reader.
    fn remaining(&self) -> usize;

    #[inline]
    fn read_u8(&mut self) -> Result<u8, DecodeError> {
        let mut buf = [0u8; 1];
        self.read_into(&mut buf)?;
        Ok(buf[0])
    }

    #[inline]
    fn read_u16(&mut self) -> Result<u16, DecodeError> {
        let mut buf = [0u8; 2];
        self.read_into(&mut buf)?;
        Ok(u16::from_le_bytes(buf))
    }

    #[inline]
    fn read_u32(&mut self) -> Result<u32, DecodeError> {
        let mut buf = [0u8; 4];
        self.read_into(&mut buf)?;
        Ok(u32::from_le_bytes(buf))
    }

    #[inline]
    fn read_u64(&mut self) -> Result<u64, DecodeError> {
        let mut buf = [0u8; 8];
        self.read_into(&mut buf)?;
        Ok(u64::from_le_bytes(buf))
    }

    #[inline]
    fn read_varint(&mut self) -> Result<u64, DecodeError>
    where
        Self: Sized,
    {
        read_varint(self)
    }

    #[inline]
    fn read_var_bytes(&mut self, max: u64) -> Result<Vec<u8>, DecodeError>
    where
        Self: Sized,
    {
        let len = self.read_varint()?;
        if len > max {
            return Err(DecodeError::LengthOutOfRange { len, max });
        }

        let mut buf = vec![0u8; len as usize];
        self.read_into(buf.as_mut_slice())?;
        Ok(buf)
    }
}

/// Reader over an in-memory byte slice.
pub struct SliceReader<'a> {
    buf: &'a [u8],
    offset: usize,
}

impl<'a> SliceReader<'a> {
    #[inline]
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, offset: 0 }
    }

    #[inline]
    pub fn consumed(&self) -> usize {
        self.offset
    }
}

impl<'a> NeoRead for SliceReader<'a> {
    fn read_into(&mut self, buf: &mut [u8]) -> Result<(), DecodeError> {
        if self.remaining() < buf.len() {
            return Err(DecodeError::UnexpectedEof {
                needed: buf.len(),
                remaining: self.remaining(),
            });
        }

        let end = self.offset + buf.len();
        buf.copy_from_slice(&self.buf[self.offset..end]);
        self.offset = end;
        Ok(())
    }

    #[inline]
    fn remaining(&self) -> usize {
        self.buf.len() - self.offset
    }
}

impl NeoWrite for Vec<u8> {
    #[inline]
    fn write_bytes(&mut self, bytes: &[u8]) {
        self.extend_from_slice(bytes);
    }

    #[inline]
    fn bytes_written(&self) -> usize {
        self.len()
    }
}

impl NeoWrite for BytesMut {
    #[inline]
    fn write_bytes(&mut self, bytes: &[u8]) {
        self.put_slice(bytes);
    }

    #[inline]
    fn bytes_written(&self) -> usize {
        BytesMut::len(self)
    }
}

impl NeoRead for Bytes {
    fn read_into(&mut self, buf: &mut [u8]) -> Result<(), DecodeError> {
        if <Bytes as Buf>::remaining(self) < buf.len() {
            return Err(DecodeError::UnexpectedEof {
                needed: buf.len(),
                remaining: <Bytes as Buf>::remaining(self),
            });
        }

        self.copy_to_slice(buf);
        Ok(())
    }

    #[inline]
    fn remaining(&self) -> usize {
        <Bytes as Buf>::remaining(self)
    }
}

impl NeoEncode for [u8] {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_var_bytes(self);
    }
}

impl NeoEncode for Vec<u8> {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_var_bytes(self);
    }
}

impl NeoDecode for Vec<u8> {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        reader.read_var_bytes(u64::MAX)
    }
}

impl NeoEncode for String {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_var_bytes(self.as_bytes());
    }
}

impl NeoEncode for str {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_var_bytes(self.as_bytes());
    }
}

impl NeoDecode for String {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let buf = reader.read_var_bytes(u32::MAX as u64)?;
        String::from_utf8(buf).map_err(|_| DecodeError::InvalidUtf8)
    }
}

impl NeoEncode for bool {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u8(*self as u8);
    }
}

impl NeoDecode for bool {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        match reader.read_u8()? {
            0 => Ok(false),
            1 => Ok(true),
            _ => Err(DecodeError::InvalidValue("bool")),
        }
    }
}

impl NeoEncode for u8 {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u8(*self);
    }
}

impl NeoDecode for u8 {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        reader.read_u8()
    }
}

impl NeoEncode for u16 {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u16(*self);
    }
}

impl NeoDecode for u16 {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        reader.read_u16()
    }
}

impl NeoEncode for u32 {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u32(*self);
    }
}

impl NeoDecode for u32 {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        reader.read_u32()
    }
}

impl NeoEncode for u64 {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_u64(*self);
    }
}

impl NeoDecode for u64 {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        reader.read_u64()
    }
}

impl NeoEncode for i64 {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_bytes(&self.to_le_bytes());
    }
}

impl NeoDecode for i64 {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let mut buf = [0u8; 8];
        reader.read_into(&mut buf)?;
        Ok(i64::from_le_bytes(buf))
    }
}

#[inline]
pub fn write_varint<W: NeoWrite + ?Sized>(writer: &mut W, value: u64) {
    let mut buf = [0u8; 9];
    let (len, data) = to_varint_le(value, &mut buf);
    writer.write_bytes(&data[..len]);
}

#[inline]
pub fn read_varint<R: NeoRead + ?Sized>(reader: &mut R) -> Result<u64, DecodeError> {
    let tag = reader.read_u8()?;
    match tag {
        value @ 0x00..=0xFC => Ok(value as u64),
        0xFD => {
            let value = reader.read_u16()?;
            if value < 0xFD {
                Err(DecodeError::InvalidVarIntTag(0xFD))
            } else {
                Ok(value as u64)
            }
        }
        0xFE => {
            let value = reader.read_u32()?;
            if value < 0x0001_0000 {
                Err(DecodeError::InvalidVarIntTag(0xFE))
            } else {
                Ok(value as u64)
            }
        }
        0xFF => {
            let value = reader.read_u64()?;
            if value < 0x0000_0001_0000_0000 {
                Err(DecodeError::InvalidVarIntTag(0xFF))
            } else {
                Ok(value)
            }
        }
    }
}

#[inline]
pub fn to_varint_le(value: u64, scratch: &mut [u8; 9]) -> (usize, [u8; 9]) {
    scratch.fill(0);
    if value < 0xFD {
        scratch[0] = value as u8;
        (1, *scratch)
    } else if value <= 0xFFFF {
        scratch[0] = 0xFD;
        scratch[1..3].copy_from_slice(&(value as u16).to_le_bytes());
        (3, *scratch)
    } else if value <= 0xFFFF_FFFF {
        scratch[0] = 0xFE;
        scratch[1..5].copy_from_slice(&(value as u32).to_le_bytes());
        (5, *scratch)
    } else {
        scratch[0] = 0xFF;
        scratch[1..9].copy_from_slice(&value.to_le_bytes());
        (9, *scratch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn varint_roundtrip() {
        let numbers = [
            0u64,
            252,
            253,
            65_535,
            65_536,
            4_294_967_295,
            4_294_967_296,
            u64::MAX,
        ];

        for value in numbers {
            let mut buf = Vec::new();
            write_varint(&mut buf, value);
            let mut reader = SliceReader::new(buf.as_slice());
            let decoded = read_varint(&mut reader).unwrap();
            assert_eq!(value, decoded);
        }
    }

    #[test]
    fn bool_encoding() {
        let mut buf = Vec::new();
        true.neo_encode(&mut buf);
        false.neo_encode(&mut buf);
        let mut reader = SliceReader::new(buf.as_slice());
        assert!(bool::neo_decode(&mut reader).unwrap());
        assert!(!bool::neo_decode(&mut reader).unwrap());
    }

    #[test]
    fn string_roundtrip() {
        let message = "neo-n3-rust";
        let mut buf = Vec::new();
        message.neo_encode(&mut buf);
        let mut reader = SliceReader::new(buf.as_slice());
        let decoded = String::neo_decode(&mut reader).unwrap();
        assert_eq!(message, decoded);
    }
}
