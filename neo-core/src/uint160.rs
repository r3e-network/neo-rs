use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;

/// Represents a 160-bit unsigned integer.
#[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct UInt160 {
    data: [u8; UInt160::LEN],
}

impl UInt160 {
    /// The length of UInt160 values.
    pub const LEN: usize = 20;

    /// Represents 0.
    pub const ZERO: Self = Self { data: [0; Self::LEN] };

    /// Creates a new UInt160 from a byte slice.
    pub fn new(bytes: &[u8]) -> Result<Self, FormatError> {
        if bytes.len() != Self::LEN {
            return Err(FormatError);
        }
        let mut data = [0u8; Self::LEN];
        data.copy_from_slice(bytes);
        Ok(Self { data })
    }

    /// Converts the UInt160 to a byte array.
    pub fn to_array(&self) -> [u8; Self::LEN] {
        self.data
    }
}

impl Serializable for UInt160 {
    type Error = io::Error;

    fn serialize(&self, writer: &mut impl io::Write) -> Result<(), Self::Error> {
        writer.write_all(&self.data)
    }

    fn deserialize(reader: &mut impl io::Read) -> Result<Self, Self::Error> {
        let mut data = [0u8; Self::LEN];
        reader.read_exact(&mut data)?;
        Ok(Self { data })
    }
}

impl fmt::Display for UInt160 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "0x{}", hex::encode(self.data.iter().rev().copied()))
    }
}

impl fmt::Debug for UInt160 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "UInt160({})", self)
    }
}

impl FromStr for UInt160 {
    type Err = ParseUInt160Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.strip_prefix("0x").unwrap_or(s);
        if s.len() != Self::LEN * 2 {
            return Err(ParseUInt160Error::InvalidLength);
        }
        let mut data = [0u8; Self::LEN];
        hex::decode_to_slice(s, &mut data).map_err(|_| ParseUInt160Error::InvalidHex)?;
        data.reverse();
        Ok(Self { data })
    }
}

#[derive(Debug)]
pub enum ParseUInt160Error {
    InvalidLength,
    InvalidHex,
}

impl From<[u8; UInt160::LEN]> for UInt160 {
    fn from(data: [u8; UInt160::LEN]) -> Self {
        Self { data }
    }
}

impl From<UInt160> for [u8; UInt160::LEN] {
    fn from(uint: UInt160) -> Self {
        uint.data
    }
}

impl AsRef<[u8]> for UInt160 {
    fn as_ref(&self) -> &[u8] {
        &self.data
    }
}

impl AsMut<[u8]> for UInt160 {
    fn as_mut(&mut self) -> &mut [u8] {
        &mut self.data
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uint160_new() {
        let data = [1u8; UInt160::LEN];
        let uint = UInt160::new(&data).unwrap();
        assert_eq!(uint.to_array(), data);
    }

    #[test]
    fn test_uint160_from_str() {
        let s = "0x0000000000000000000000000000000000000001";
        let uint = UInt160::from_str(s).unwrap();
        assert_eq!(uint.to_array(), [1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn test_uint160_display() {
        let uint = UInt160::from([1u8; UInt160::LEN]);
        assert_eq!(uint.to_string(), "0x0101010101010101010101010101010101010101");
    }

    #[test]
    fn test_uint160_serialization() {
        let uint = UInt160::from([1u8; UInt160::LEN]);
        let mut writer = Vec::new();
        uint.serialize(&mut writer).unwrap();
        let deserialized = UInt160::deserialize(&mut &writer[..]).unwrap();
        assert_eq!(uint, deserialized);
    }
}
