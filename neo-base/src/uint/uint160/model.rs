use alloc::{string::String, vec::Vec};

use crate::encoding::{DecodeError, FromBase58Check, StartsWith0x, ToBase58Check};

use super::super::address::{AddressError, AddressVersion};

pub(super) const U160_LEN: usize = 20;

/// Neo's 160-bit unsigned integer (little-endian in memory, big-endian when formatted).
#[derive(Clone, Copy, Eq, PartialEq, Hash, Default)]
pub struct UInt160(pub(super) [u8; U160_LEN]);

impl UInt160 {
    pub const LENGTH: usize = U160_LEN;
    pub const ZERO: Self = Self([0u8; U160_LEN]);

    #[inline]
    pub const fn new(bytes: [u8; U160_LEN]) -> Self {
        Self(bytes)
    }

    #[inline]
    pub fn from_slice(slice: &[u8]) -> Result<Self, DecodeError> {
        if slice.len() != U160_LEN {
            return Err(DecodeError::LengthOutOfRange {
                len: slice.len() as u64,
                max: U160_LEN as u64,
            });
        }
        let mut buf = [0u8; U160_LEN];
        buf.copy_from_slice(slice);
        Ok(Self(buf))
    }

    #[inline]
    pub fn to_array(self) -> [u8; U160_LEN] {
        self.0
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8; U160_LEN] {
        &self.0
    }

    #[inline]
    pub fn to_vec(self) -> Vec<u8> {
        self.0.to_vec()
    }

    pub fn to_address(&self, version: AddressVersion) -> String {
        let mut payload = Vec::with_capacity(1 + U160_LEN);
        payload.push(version.0);
        payload.extend_from_slice(&self.0);
        payload.to_base58_check()
    }

    pub fn from_address(address: &str, version: AddressVersion) -> Result<Self, AddressError> {
        let decoded = Vec::<u8>::from_base58_check(address)?;
        if decoded.len() != 1 + U160_LEN {
            return Err(AddressError::InvalidLength {
                length: decoded.len(),
            });
        }
        if decoded[0] != version.0 {
            return Err(AddressError::InvalidVersion {
                expected: version.0,
                found: decoded[0],
            });
        }
        let mut buf = [0u8; U160_LEN];
        buf.copy_from_slice(&decoded[1..]);
        Ok(Self(buf))
    }

    #[inline]
    pub fn from_script(script: &[u8]) -> Self {
        let hash = crate::hash::hash160(script);
        Self::from_slice(&hash).expect("hash160 output size is 20 bytes")
    }

    pub fn from_hex_str(value: &str) -> Result<Self, DecodeError> {
        let trimmed = value.trim();
        let without_prefix = if trimmed.as_bytes().starts_with_0x() {
            &trimmed[2..]
        } else {
            trimmed
        };
        if without_prefix.len() != U160_LEN * 2 {
            return Err(DecodeError::LengthOutOfRange {
                len: without_prefix.len() as u64 / 2,
                max: U160_LEN as u64,
            });
        }
        let bytes =
            hex::decode(without_prefix).map_err(|_| DecodeError::InvalidValue("UInt160 hex"))?;
        let mut array = [0u8; U160_LEN];
        array.copy_from_slice(&bytes);
        array.reverse();
        Ok(Self(array))
    }

    #[inline]
    pub fn is_zero(&self) -> bool {
        self.0.iter().all(|b| *b == 0)
    }
}
