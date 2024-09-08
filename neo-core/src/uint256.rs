use std::cmp::Ordering;
use std::fmt;
use std::hash::Hasher;
use std::io::Write;
use std::str::FromStr;
use byteorder::LittleEndian;
use crate::io::iserializable::ISerializable;

/// Represents a 256-bit unsigned integer.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UInt256 {
    data: [u64; 4],
}

impl UInt256 {
    /// Creates a new UInt256 from a slice of bytes.
    ///
    /// # Arguments
    ///
    /// * `slice` - A slice of bytes representing the UInt256 value.
    ///
    /// # Panics
    ///
    /// Panics if the slice length is not equal to `LENGTH`.
    pub(crate) fn from_slice(slice: &[u8]) -> UInt256 {
        assert_eq!(slice.len(), Self::LENGTH, "Invalid byte length for UInt256");
        let mut data = [0u64; 4];
        for (i, chunk) in slice.chunks_exact(8).enumerate() {
            data[i] = u64::from_le_bytes(chunk.try_into().unwrap());
        }
        Self { data }
    }
}

impl UInt256 {
    /// The length of UInt256 values in bytes.
    pub const LENGTH: usize = 32;

    /// Represents 0.
    pub const ZERO: Self = Self { data: [0; 4] };

    /// Creates a new UInt256 from a byte slice.
    ///
    /// # Panics
    ///
    /// Panics if the slice length is not equal to `LENGTH`.
    pub fn new(bytes: &[u8]) -> Self {
        assert_eq!(bytes.len(), Self::LENGTH, "Invalid byte length for UInt256");
        let mut data = [0u64; 4];
        for (i, chunk) in bytes.chunks_exact(8).enumerate() {
            data[i] = u64::from_le_bytes(chunk.try_into().unwrap());
        }
        Self { data }
    }

    /// Converts the UInt256 to a byte array.
    pub fn to_array(&self) -> [u8; Self::LENGTH] {
        let mut result = [0u8; Self::LENGTH];
        for (i, &value) in self.data.iter().enumerate() {
            result[i * 8..(i + 1) * 8].copy_from_slice(&value.to_le_bytes());
        }
        result
    }
}

impl ISerializable for UInt256 {
    fn size(&self) -> usize {
        todo!()
    }

    fn serialize(&self, writer: &mut impl Write) -> IoResult<()> {
        for &value in &self.data {
            writer.write_u64::<LittleEndian>(value)?;
        }
        Ok(())
    }

    fn deserialize(reader: &mut impl Read) -> IoResult<Self> {
        let mut data = [0u64; 4];
        for value in &mut data {
            *value = reader.read_u64::<LittleEndian>()?;
        }
        Ok(Self { data })
    }
}

impl fmt::Display for UInt256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(self.to_array().iter().rev().copied()))
    }
}

impl fmt::Debug for UInt256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UInt256({})", self)
    }
}

impl FromStr for UInt256 {
    type Err = ParseUInt256Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix("0x").unwrap_or(s);
        if s.len() != Self::LENGTH * 2 {
            return Err(ParseUInt256Error::InvalidLength);
        }
        let bytes = hex::decode(s).map_err(|_| ParseUInt256Error::InvalidHex)?;
        Ok(Self::new(&bytes))
    }
}

#[derive(Debug)]
pub enum ParseUInt256Error {
    InvalidLength,
    InvalidHex,
}

impl fmt::Display for ParseUInt256Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidLength => write!(f, "invalid UInt256 length"),
            Self::InvalidHex => write!(f, "invalid hexadecimal string"),
        }
    }
}

impl std::error::Error for ParseUInt256Error {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uint256_zero() {
        assert_eq!(UInt256::ZERO.to_array(), [0u8; 32]);
    }

    #[test]
    fn test_uint256_from_bytes() {
        let bytes = [1u8; 32];
        let uint = UInt256::new(&bytes);
        assert_eq!(uint.to_array(), bytes);
    }

    #[test]
    fn test_uint256_serialization() {
        let uint = UInt256::new(&[1u8; 32]);
        let mut writer = Vec::new();
        uint.serialize(&mut writer).unwrap();
        let deserialized = UInt256::deserialize(&mut &writer[..]).unwrap();
        assert_eq!(uint, deserialized);
    }

    #[test]
    fn test_uint256_display() {
        let uint = UInt256::new(&[0xFFu8; 32]);
        assert_eq!(
            uint.to_string(),
            "0xffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffffff"
        );
    }

    #[test]
    fn test_uint256_from_str() {
        let s = "0x0102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f20";
        let uint = UInt256::from_str(s).unwrap();
        assert_eq!(uint.to_string(), s);
    }

    #[test]
    fn test_uint256_ordering() {
        let a = UInt256::new(&[1u8; 32]);
        let b = UInt256::new(&[2u8; 32]);
        assert!(a < b);
    }
}
