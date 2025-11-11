use alloc::fmt;

use neo_base::encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite, ToHex};
use p256::ecdsa::Signature;

pub const SIGNATURE_SIZE: usize = 64;

#[derive(Clone, Copy, Eq, PartialEq, Hash)]
pub struct SignatureBytes(pub [u8; SIGNATURE_SIZE]);

impl SignatureBytes {
    #[inline]
    pub fn as_ref(&self) -> &[u8; SIGNATURE_SIZE] {
        &self.0
    }
}

impl fmt::Debug for SignatureBytes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SignatureBytes")
            .field(&self.0.to_hex_lower())
            .finish()
    }
}

impl NeoEncode for SignatureBytes {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        writer.write_bytes(&self.0);
    }
}

impl NeoDecode for SignatureBytes {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let mut buf = [0u8; SIGNATURE_SIZE];
        reader.read_into(&mut buf)?;
        Ok(SignatureBytes(buf))
    }
}

impl TryFrom<SignatureBytes> for Signature {
    type Error = super::VerifyError;

    #[inline]
    fn try_from(value: SignatureBytes) -> Result<Self, Self::Error> {
        Signature::try_from(value.0.as_slice()).map_err(|_| super::VerifyError::InvalidSignature)
    }
}

impl From<Signature> for SignatureBytes {
    #[inline]
    fn from(value: Signature) -> Self {
        SignatureBytes(value.to_bytes().into())
    }
}
