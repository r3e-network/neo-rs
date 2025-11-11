use alloc::fmt::{self, Debug, Formatter};

use neo_base::{
    encoding::{DecodeError, NeoDecode, NeoEncode, NeoRead, NeoWrite, ToHex},
    hash::{hash160, Hash160},
};
use p256::{
    elliptic_curve::sec1::{FromEncodedPoint, ToEncodedPoint},
    AffinePoint, EncodedPoint,
};

use super::KEY_SIZE;

#[derive(Clone, Eq, PartialEq, Hash)]
pub struct PublicKey {
    pub(super) gx: [u8; KEY_SIZE],
    pub(super) gy: [u8; KEY_SIZE],
}

impl PublicKey {
    #[inline]
    pub fn from_affine(point: AffinePoint) -> Self {
        let encoded = point.to_encoded_point(false);
        let mut gx = [0u8; KEY_SIZE];
        let mut gy = [0u8; KEY_SIZE];
        let x = encoded.x().expect("x coordinate");
        let y = encoded.y().expect("y coordinate");
        gx.copy_from_slice(x.as_ref());
        gy.copy_from_slice(y.as_ref());
        Self { gx, gy }
    }

    #[inline]
    pub fn from_sec1_bytes(bytes: &[u8]) -> Result<Self, super::KeyError> {
        let encoded =
            EncodedPoint::from_bytes(bytes).map_err(|_| super::KeyError::InvalidPublicKey)?;
        let point = Option::<AffinePoint>::from(AffinePoint::from_encoded_point(&encoded))
            .ok_or(super::KeyError::InvalidPublicKey)?;
        Ok(Self::from_affine(point))
    }

    #[inline]
    pub fn to_uncompressed(&self) -> [u8; 65] {
        let mut buf = [0u8; 65];
        buf[0] = 0x04;
        buf[1..33].copy_from_slice(&self.gx);
        buf[33..].copy_from_slice(&self.gy);
        buf
    }

    #[inline]
    pub fn to_compressed(&self) -> [u8; 33] {
        let mut buf = [0u8; 33];
        buf[0] = 0x02 + (self.gy[KEY_SIZE - 1] & 0x01);
        buf[1..].copy_from_slice(&self.gx);
        buf
    }

    #[inline]
    pub fn signature_redeem_script(&self) -> [u8; 35] {
        let mut script = [0u8; 35];
        script[0] = 0x21; // PUSHBYTES33
        script[1..34].copy_from_slice(&self.to_compressed());
        script[34] = 0xAC; // CHECKSIG
        script
    }

    #[inline]
    pub fn script_hash(&self) -> Hash160 {
        Hash160::from_slice(&hash160(self.signature_redeem_script())).expect("hash160 length is 20")
    }
}

impl Debug for PublicKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("PublicKey")
            .field("compressed", &self.to_compressed().to_hex_lower())
            .finish()
    }
}

impl NeoEncode for PublicKey {
    #[inline]
    fn neo_encode<W: NeoWrite>(&self, writer: &mut W) {
        let compressed = self.to_compressed();
        writer.write_var_bytes(&compressed);
    }
}

impl NeoDecode for PublicKey {
    #[inline]
    fn neo_decode<R: NeoRead>(reader: &mut R) -> Result<Self, DecodeError> {
        let bytes = reader.read_var_bytes(65)?;
        PublicKey::from_sec1_bytes(&bytes).map_err(|_| DecodeError::InvalidValue("PublicKey"))
    }
}
