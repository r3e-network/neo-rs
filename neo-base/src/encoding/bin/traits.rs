use alloc::{vec, vec::Vec};

use super::{read_varint, write_varint, DecodeError};

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
