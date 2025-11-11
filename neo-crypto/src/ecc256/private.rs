use alloc::fmt::{self, Debug, Formatter};

use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite};
use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

use super::KEY_SIZE;

#[derive(Clone)]
pub struct PrivateKey {
    key: Zeroizing<[u8; KEY_SIZE]>,
}

impl PrivateKey {
    #[inline]
    pub fn new(bytes: [u8; KEY_SIZE]) -> Self {
        Self {
            key: Zeroizing::new(bytes),
        }
    }

    #[inline]
    pub fn from_slice(slice: &[u8]) -> Result<Self, super::KeyError> {
        if slice.len() != KEY_SIZE {
            return Err(super::KeyError::InvalidPrivateKeyLength);
        }
        let mut buf = [0u8; KEY_SIZE];
        buf.copy_from_slice(slice);
        Ok(Self::new(buf))
    }

    #[inline]
    pub fn as_be_bytes(&self) -> &[u8] {
        self.key.as_slice()
    }
}

impl Debug for PrivateKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_tuple("PrivateKey").field(&"***").finish()
    }
}

impl Eq for PrivateKey {}

impl PartialEq for PrivateKey {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.key.as_slice().ct_eq(other.key.as_slice()).into()
    }
}

impl PartialEq<[u8]> for PrivateKey {
    #[inline]
    fn eq(&self, other: &[u8]) -> bool {
        self.key.as_slice().ct_eq(other).into()
    }
}

impl NeoEncode for PrivateKey {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_bytes(self.as_be_bytes());
    }
}

impl NeoDecode for PrivateKey {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let mut buf = [0u8; KEY_SIZE];
        reader.read_into(&mut buf)?;
        Ok(PrivateKey::new(buf))
    }
}
