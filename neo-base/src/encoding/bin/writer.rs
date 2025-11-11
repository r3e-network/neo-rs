use alloc::vec::Vec;

use bytes::{BufMut, BytesMut};

use super::NeoWrite;

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
