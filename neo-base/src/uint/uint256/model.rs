use crate::encoding::{DecodeError, StartsWith0x};

pub(super) const U256_LEN: usize = 32;

/// Neo's 256-bit unsigned integer.
#[derive(Clone, Copy, Eq, PartialEq, Hash, Default)]
pub struct UInt256(pub(super) [u8; U256_LEN]);

impl UInt256 {
    pub const LENGTH: usize = U256_LEN;
    pub const ZERO: Self = Self([0u8; U256_LEN]);

    #[inline]
    pub const fn new(bytes: [u8; U256_LEN]) -> Self {
        Self(bytes)
    }

    #[inline]
    pub fn from_slice(slice: &[u8]) -> Result<Self, DecodeError> {
        if slice.len() != U256_LEN {
            return Err(DecodeError::LengthOutOfRange {
                len: slice.len() as u64,
                max: U256_LEN as u64,
            });
        }
        let mut buf = [0u8; U256_LEN];
        buf.copy_from_slice(slice);
        Ok(Self(buf))
    }

    #[inline]
    pub fn as_slice(&self) -> &[u8; U256_LEN] {
        &self.0
    }

    #[inline]
    pub fn to_array(self) -> [u8; U256_LEN] {
        self.0
    }

    pub fn from_hex_str(value: &str) -> Result<Self, DecodeError> {
        let trimmed = value.trim();
        let without_prefix = if trimmed.as_bytes().starts_with_0x() {
            &trimmed[2..]
        } else {
            trimmed
        };
        if without_prefix.len() != U256_LEN * 2 {
            return Err(DecodeError::LengthOutOfRange {
                len: without_prefix.len() as u64 / 2,
                max: U256_LEN as u64,
            });
        }
        let bytes =
            hex::decode(without_prefix).map_err(|_| DecodeError::InvalidValue("UInt256 hex"))?;
        let mut array = [0u8; U256_LEN];
        array.copy_from_slice(&bytes);
        array.reverse();
        Ok(Self(array))
    }
}
