use core::fmt::{Display, Formatter};

use neo_base::encoding::ToRevHex;

pub const H256_SIZE: usize = 32;

#[derive(Debug, Copy, Clone, Hash, Eq, PartialEq)]
pub struct H256([u8; H256_SIZE]);

impl H256 {
    #[inline]
    pub fn from_le_bytes(src: [u8; H256_SIZE]) -> Self {
        H256(src)
    }

    #[inline]
    pub fn as_le_bytes(&self) -> &[u8] {
        &self.0
    }
}

impl AsRef<[u8; H256_SIZE]> for H256 {
    #[inline]
    fn as_ref(&self) -> &[u8; H256_SIZE] {
        &self.0
    }
}

impl AsRef<[u8]> for H256 {
    #[inline]
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl Display for H256 {
    #[inline]
    fn fmt(&self, f: &mut Formatter<'_>) -> core::fmt::Result {
        f.write_str("0x")?;
        f.write_str(&self.0.to_rev_hex_lower())
    }
}

impl Default for H256 {
    #[inline]
    fn default() -> Self {
        Self([0u8; H256_SIZE])
    }
}
