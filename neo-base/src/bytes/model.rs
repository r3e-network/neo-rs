use alloc::{borrow::Cow, fmt, vec::Vec};
use core::{ops::Deref, slice};

use crate::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};

/// Heap allocated byte buffer with `NeoEncode`/`NeoDecode` support.
#[derive(Clone, Eq, PartialEq, Hash)]
pub struct Bytes(pub(crate) Vec<u8>);

impl Bytes {
    #[inline]
    pub fn new(vec: Vec<u8>) -> Self {
        Self(vec)
    }

    #[inline]
    pub fn len(&self) -> usize {
        self.0.len()
    }

    #[inline]
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8] {
        self.0.as_slice()
    }

    #[inline]
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }

    #[inline]
    pub fn as_vec(&self) -> &Vec<u8> {
        &self.0
    }
}

impl From<Vec<u8>> for Bytes {
    #[inline]
    fn from(value: Vec<u8>) -> Self {
        Bytes(value)
    }
}

impl From<&[u8]> for Bytes {
    #[inline]
    fn from(value: &[u8]) -> Self {
        Bytes(value.to_vec())
    }
}

impl From<Bytes> for Vec<u8> {
    #[inline]
    fn from(value: Bytes) -> Self {
        value.0
    }
}

impl Default for Bytes {
    #[inline]
    fn default() -> Self {
        Self(Vec::new())
    }
}

impl Deref for Bytes {
    type Target = [u8];

    #[inline]
    fn deref(&self) -> &Self::Target {
        self.0.as_slice()
    }
}

impl fmt::Debug for Bytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if f.alternate() {
            write!(f, "Bytes(0x{})", hex::encode(&self.0))
        } else {
            f.debug_tuple("Bytes").field(&self.0).finish()
        }
    }
}

impl NeoEncode for Bytes {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_var_bytes(&self.0);
    }
}

impl NeoDecode for Bytes {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        Ok(Bytes(reader.read_var_bytes(u32::MAX as u64)?))
    }
}

impl<'a> From<Cow<'a, [u8]>> for Bytes {
    #[inline]
    fn from(value: Cow<'a, [u8]>) -> Self {
        match value {
            Cow::Borrowed(slice) => Bytes(slice.to_vec()),
            Cow::Owned(vec) => Bytes(vec),
        }
    }
}

impl<'a> IntoIterator for &'a Bytes {
    type Item = &'a u8;
    type IntoIter = slice::Iter<'a, u8>;

    #[inline]
    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}
