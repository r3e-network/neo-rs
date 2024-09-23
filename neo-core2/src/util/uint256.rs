use std::cmp::Ordering;
use std::fmt;
use std::str::FromStr;
use hex;
use serde::{Deserialize, Serialize};

// Uint256Size is the size of Uint256 in bytes.
pub const UINT256_SIZE: usize = 32;

// Uint256 is a 32 byte long unsigned integer.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct Uint256([u8; UINT256_SIZE]);

impl Uint256 {
    // Uint256DecodeStringLE attempts to decode the given string (in LE representation) into a Uint256.
    pub fn from_str_le(s: &str) -> Result<Self, String> {
        if s.len() != UINT256_SIZE * 2 {
            return Err(format!("expected string size of {} got {}", UINT256_SIZE * 2, s.len()));
        }
        let mut bytes = hex::decode(s).map_err(|e| e.to_string())?;
        bytes.reverse();
        Self::from_be_bytes(&bytes)
    }

    // Uint256DecodeStringBE attempts to decode the given string (in BE representation) into a Uint256.
    pub fn from_str_be(s: &str) -> Result<Self, String> {
        if s.len() != UINT256_SIZE * 2 {
            return Err(format!("expected string size of {} got {}", UINT256_SIZE * 2, s.len()));
        }
        let bytes = hex::decode(s).map_err(|e| e.to_string())?;
        Self::from_be_bytes(&bytes)
    }

    // Uint256DecodeBytesBE attempts to decode the given bytes (in BE representation) into a Uint256.
    pub fn from_be_bytes(b: &[u8]) -> Result<Self, String> {
        if b.len() != UINT256_SIZE {
            return Err(format!("expected []byte of size {} got {}", UINT256_SIZE, b.len()));
        }
        let mut arr = [0u8; UINT256_SIZE];
        arr.copy_from_slice(b);
        Ok(Uint256(arr))
    }

    // Uint256DecodeBytesLE attempts to decode the given bytes (in LE representation) into a Uint256.
    pub fn from_le_bytes(b: &[u8]) -> Result<Self, String> {
        if b.len() != UINT256_SIZE {
            return Err(format!("expected []byte of size {} got {}", UINT256_SIZE, b.len()));
        }
        let mut arr = [0u8; UINT256_SIZE];
        arr.copy_from_slice(b);
        arr.reverse();
        Ok(Uint256(arr))
    }

    // BytesBE returns a byte slice representation of u.
    pub fn to_be_bytes(&self) -> [u8; UINT256_SIZE] {
        self.0
    }

    // Reverse reverses the Uint256 object.
    pub fn reverse(&self) -> Self {
        let mut reversed = self.0;
        reversed.reverse();
        Uint256(reversed)
    }

    // BytesLE return a little-endian byte representation of u.
    pub fn to_le_bytes(&self) -> [u8; UINT256_SIZE] {
        let mut le = self.0;
        le.reverse();
        le
    }

    // StringBE produces string representation of Uint256 with BE byte order.
    pub fn to_string_be(&self) -> String {
        hex::encode(self.0)
    }

    // StringLE produces string representation of Uint256 with LE byte order.
    pub fn to_string_le(&self) -> String {
        hex::encode(self.to_le_bytes())
    }
}

impl fmt::Display for Uint256 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_string_be())
    }
}

impl FromStr for Uint256 {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uint256::from_str_le(s)
    }
}

impl Serialize for Uint256 {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let s = format!("0x{}", self.to_string_le());
        serializer.serialize_str(&s)
    }
}

impl<'de> Deserialize<'de> for Uint256 {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let s = s.trim_start_matches("0x");
        Uint256::from_str_le(s).map_err(serde::de::Error::custom)
    }
}

impl Ord for Uint256 {
    fn cmp(&self, other: &Self) -> Ordering {
        self.0.cmp(&other.0)
    }
}

impl PartialOrd for Uint256 {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// Note: The io::Serializable trait is not a standard Rust trait.
// You might need to implement a custom trait or use a crate like `bincode` for binary serialization.
