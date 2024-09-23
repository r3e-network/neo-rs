use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;
use hex;
use serde::{Deserialize, Serialize};

/// Size of Uint160 in bytes.
pub const UINT160_SIZE: usize = 20;

/// Uint160 is a 20 byte long unsigned integer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct Uint160([u8; UINT160_SIZE]);

impl Uint160 {
    /// Attempts to decode the given string into a Uint160 in big-endian format.
    pub fn from_str_be(s: &str) -> Result<Self, Box<dyn std::error::Error>> {
        if s.len() != UINT160_SIZE * 2 {
            return Err(format!("expected string size of {} got {}", UINT160_SIZE * 2, s.len()).into());
        }
        let bytes = hex::decode(s)?;
        Self::from_slice_be(&bytes)
    }

    /// Attempts to decode the given string into a Uint160 in little-endian format.
    pub fn from_str_le(s: &str) -> Result<Self, Box<dyn std::error::Error>> {
        if s.len() != UINT160_SIZE * 2 {
            return Err(format!("expected string size of {} got {}", UINT160_SIZE * 2, s.len()).into());
        }
        let bytes = hex::decode(s)?;
        Self::from_slice_le(&bytes)
    }

    /// Attempts to decode the given bytes into a Uint160 in big-endian format.
    pub fn from_slice_be(b: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        if b.len() != UINT160_SIZE {
            return Err(format!("expected byte size of {} got {}", UINT160_SIZE, b.len()).into());
        }
        let mut arr = [0u8; UINT160_SIZE];
        arr.copy_from_slice(b);
        Ok(Uint160(arr))
    }

    /// Attempts to decode the given bytes into a Uint160 in little-endian format.
    pub fn from_slice_le(b: &[u8]) -> Result<Self, Box<dyn std::error::Error>> {
        if b.len() != UINT160_SIZE {
            return Err(format!("expected byte size of {} got {}", UINT160_SIZE, b.len()).into());
        }
        let mut arr = [0u8; UINT160_SIZE];
        for i in 0..UINT160_SIZE {
            arr[UINT160_SIZE - i - 1] = b[i];
        }
        Ok(Uint160(arr))
    }

    /// Returns a big-endian byte representation.
    pub fn to_be_bytes(&self) -> [u8; UINT160_SIZE] {
        self.0
    }

    /// Returns a little-endian byte representation.
    pub fn to_le_bytes(&self) -> [u8; UINT160_SIZE] {
        let mut arr = self.0;
        arr.reverse();
        arr
    }

    /// Returns a string representation in big-endian format.
    pub fn to_string_be(&self) -> String {
        hex::encode(self.to_be_bytes())
    }

    /// Returns a string representation in little-endian format.
    pub fn to_string_le(&self) -> String {
        hex::encode(self.to_le_bytes())
    }

    /// Returns a reversed representation.
    pub fn reverse(&self) -> Self {
        let mut arr = self.0;
        arr.reverse();
        Uint160(arr)
    }

    /// Compares this Uint160 with another.
    pub fn compare(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl fmt::Display for Uint160 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string_be())
    }
}

impl FromStr for Uint160 {
    type Err = Box<dyn std::error::Error>;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uint160::from_str_le(s)
    }
}

impl PartialOrd for Uint160 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Uint160 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.compare(other)
    }
}

impl Serialize for Uint160 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = format!("0x{}", self.to_string_le());
        serializer.serialize_str(&s)
    }
}

impl<'de> Deserialize<'de> for Uint160 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let s = s.trim_start_matches("0x");
        Uint160::from_str_le(s).map_err(serde::de::Error::custom)
    }
}

// Note: The binary encoding and decoding methods (EncodeBinary and DecodeBinary) 
// are not directly translated as they depend on the `io` package from the original Go code.
// You might want to implement these using a similar binary reading/writing mechanism in Rust.
