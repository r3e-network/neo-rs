use bytes::{Buf, Bytes};

use super::{DecodeError, NeoRead};

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
