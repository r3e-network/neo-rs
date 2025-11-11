use core::fmt::{Display, Formatter};

use neo_base::encoding::ToRevHex;

pub const H160_SIZE: usize = 20;

/// Little-endian 160-bit hash.
#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
#[repr(align(8))]
pub struct H160([u8; H160_SIZE]);

impl H160 {
    #[inline]
    pub fn from_le_bytes(src: [u8; H160_SIZE]) -> Self {
        H160(src)
    }

    #[inline]
    pub fn as_le_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8; H160_SIZE]> for H160 {
    #[inline]
    fn as_ref(&self) -> &[u8; H160_SIZE] {
        &self.0
    }
}

impl AsRef<[u8]> for H160 {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Display for H160 {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str("0x")?;
        f.write_str(&self.0.to_rev_hex_lower())
    }
}

impl Default for H160 {
    #[inline]
    fn default() -> Self {
        Self([0u8; H160_SIZE])
    }
}
