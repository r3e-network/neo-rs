// Copyright @ 2025 - present, R3E Network
// All Rights Reserved

use subtle::ConstantTimeEq;
use zeroize::Zeroizing;

pub const KEY_SIZE: usize = 32;

#[derive(Debug, Clone)]
pub struct PrivateKey {
    // key is in big endian
    key: Zeroizing<[u8; KEY_SIZE]>,
}

impl PrivateKey {
    #[inline]
    pub fn new(big_endian_key: Zeroizing<[u8; KEY_SIZE]>) -> Self {
        Self {
            key: big_endian_key,
        }
    }

    #[inline]
    pub fn as_be_bytes(&self) -> &[u8] {
        self.key.as_slice()
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

#[derive(Debug, Clone, Hash, PartialEq, Eq)]
pub struct PublicKey {
    // gx, gy are in big endian
    gx: [u8; KEY_SIZE],
    gy: [u8; KEY_SIZE],
}

impl PublicKey {
    pub fn to_uncompressed(&self) -> [u8; 2 * KEY_SIZE + 1] {
        let mut buf = [0u8; 2 * KEY_SIZE + 1];
        buf[0] = 0x04;
        buf[1..1 + KEY_SIZE].copy_from_slice(self.gx.as_slice());
        buf[1 + KEY_SIZE..].copy_from_slice(self.gy.as_slice());
        buf
    }

    pub fn to_compressed(&self) -> [u8; KEY_SIZE + 1] {
        let mut buf = [0u8; KEY_SIZE + 1];
        buf[0] = 0x02 + (self.gy[KEY_SIZE - 1] & 0x01); // 0x02 when y is even, 0x03 when y is odd
        buf[1..].copy_from_slice(self.gx.as_slice());
        buf
    }
}

#[derive(Clone)]
pub struct Keypair {
    pub private_key: PrivateKey,
    pub public_key: PublicKey,
}
