//! Implementation of `UInt160`, a 160-bit unsigned integer.

use crate::constants::ADDRESS_SIZE;
use crate::{base58_check, base58_check::AddressDecodeError};
use ripemd::Ripemd160;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

/// The length of `UInt160` values in bytes.
pub const UINT160_SIZE: usize = ADDRESS_SIZE;

crate::uint_type! {
    /// Represents a 160-bit unsigned integer.
    #[derive(Clone, Copy, Default, Eq, PartialEq, Hash, Serialize, Deserialize)]
    #[repr(C)]
    pub struct UInt160 {
        size = UINT160_SIZE;
        size_const = UINT160_SIZE;
        /// Zero value for `UInt160`.
        ZERO;
        as_ref = false;
        fields: [value1: u64, value2: u64, value3: u32];
    }
}

impl UInt160 {
    /// Creates a `UInt160` from a script by computing its hash.
    #[must_use]
    pub fn from_script(script: &[u8]) -> Self {
        let mut sha256_hasher = Sha256::new();
        sha256_hasher.update(script);
        let sha256_hash = sha256_hasher.finalize();

        let mut ripemd_hasher = Ripemd160::new();
        ripemd_hasher.update(sha256_hash);
        let hash160 = ripemd_hasher.finalize();

        Self::from_bytes(&hash160).unwrap_or_default()
    }

    /// Converts this `UInt160` to a Neo address string.
    #[must_use]
    pub fn to_address(&self) -> String {
        base58_check::encode_address_payload(crate::constants::ADDRESS_VERSION, &self.to_array())
    }

    /// Converts this `UInt160` to a Neo address string using an explicit
    /// address-version byte (matches C# `ToAddress(byte version)`).
    #[must_use]
    pub fn to_address_with_version(&self, version: u8) -> String {
        base58_check::encode_address_payload(version, &self.to_array())
    }

    /// Parses a Neo address string to a `UInt160`.
    ///
    /// # Errors
    ///
    /// Returns `PrimitiveError::InvalidFormat` if the address is not valid Base58,
    /// has an incorrect length, has an invalid version byte, or has an invalid checksum.
    pub fn from_address(address: &str) -> crate::PrimitiveResult<Self> {
        let script_hash =
            base58_check::decode_address_payload(address, crate::constants::ADDRESS_VERSION)
                .map_err(map_base58_check_address_error)?;
        Self::from_bytes(&script_hash)
    }

    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::cast_possible_wrap)]
    #[must_use]
    /// Returns the Neo-compatible 32-bit hash code for this value.
    pub const fn hash_code(&self) -> i32 {
        let v1_hash = (self.value1 as i32) ^ ((self.value1 >> 32) as i32);
        let v2_hash = (self.value2 as i32) ^ ((self.value2 >> 32) as i32);
        let v3_hash = self.value3 as i32;
        let mut hash = 17i32;
        hash = hash.wrapping_mul(31).wrapping_add(v1_hash);
        hash = hash.wrapping_mul(31).wrapping_add(v2_hash);
        hash = hash.wrapping_mul(31).wrapping_add(v3_hash);
        hash
    }
}

fn map_base58_check_address_error(error: AddressDecodeError) -> crate::PrimitiveError {
    let message = match error {
        AddressDecodeError::Base58(base58_check::Base58CheckDecodeError::InvalidChecksum) => {
            "Invalid address checksum"
        }
        AddressDecodeError::Base58(base58_check::Base58CheckDecodeError::MissingChecksum)
        | AddressDecodeError::InvalidLength { .. } => "Invalid address length",
        AddressDecodeError::Base58(base58_check::Base58CheckDecodeError::InvalidBase58 {
            ..
        }) => "Invalid Base58 address",
        AddressDecodeError::InvalidVersion { .. } => "Invalid address version",
    };
    crate::PrimitiveError::InvalidFormat {
        message: message.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_uint160_new() {
        let uint = UInt160::new();
        assert_eq!(uint.value1, 0);
        assert_eq!(uint.value2, 0);
        assert_eq!(uint.value3, 0);
    }

    #[test]
    fn test_uint160_from_bytes() {
        let mut bytes = [0u8; ADDRESS_SIZE];
        bytes[0] = 1;
        let uint = UInt160::from_bytes(&bytes).unwrap();
        assert_eq!(uint.value1, 1);
        assert_eq!(uint.value2, 0);
        assert_eq!(uint.value3, 0);
    }

    #[test]
    fn test_uint160_to_array() {
        let mut uint = UInt160::new();
        uint.value1 = 1;
        let bytes = uint.to_array();
        assert_eq!(bytes[0], 1);
        assert_eq!(bytes[1], 0);
    }

    #[test]
    fn test_uint160_parse() {
        let hex_str = "0x0000000000000000000000000000000000000001";
        let uint = UInt160::parse(hex_str).unwrap();
        assert_eq!(uint.value1, 1);
        assert_eq!(uint.value2, 0);
        assert_eq!(uint.value3, 0);
    }

    #[test]
    fn test_uint160_to_hex_string() {
        let mut uint = UInt160::new();
        uint.value3 = 0x01000000;
        let hex_str = uint.to_hex_string();
        assert_eq!(hex_str, "0x0100000000000000000000000000000000000000");
    }

    #[test]
    fn test_uint160_ordering() {
        let uint1 = UInt160 {
            value1: 1,
            value2: 0,
            value3: 0,
        };
        let uint2 = UInt160 {
            value1: 0,
            value2: 1,
            value3: 0,
        };
        let uint3 = UInt160 {
            value1: 0,
            value2: 0,
            value3: 1,
        };
        assert!(uint3 > uint2);
        assert!(uint2 > uint1);
        assert!(uint3 > uint1);
    }

    #[test]
    fn test_uint160_equals() {
        let uint1 = UInt160 {
            value1: 1,
            value2: 2,
            value3: 3,
        };
        let uint2 = UInt160 {
            value1: 1,
            value2: 2,
            value3: 3,
        };
        let uint3 = UInt160 {
            value1: 1,
            value2: 2,
            value3: 4,
        };
        assert!(uint1.equals(Some(&uint2)));
        assert!(!uint1.equals(Some(&uint3)));
        assert!(!uint1.equals(None));
    }

    #[test]
    fn test_uint160_from_script() {
        let script = b"Hello, Neo!";
        let uint = UInt160::from_script(script);
        assert!(!uint.is_zero());
    }

    #[test]
    fn address_uses_base58_check_roundtrip() {
        let script_hash = UInt160::from_bytes(&[0x42; UINT160_SIZE]).unwrap();
        let address = script_hash.to_address();
        let payload = bs58::decode(&address).with_check(None).into_vec().unwrap();
        assert_eq!(payload.len(), 1 + UINT160_SIZE);
        assert_eq!(payload[0], crate::constants::ADDRESS_VERSION);
        assert_eq!(&payload[1..], script_hash.to_array());
        assert_eq!(UInt160::from_address(&address).unwrap(), script_hash);
    }

    #[test]
    fn address_matches_known_neo_vectors() {
        let vectors = [
            (
                "0xb8a020fce295c9e36ab7ec3502c9ebbabf2d8878",
                "NWuHQdxabXPdC6vVwJhxjYELDQPqc1d4TG",
            ),
            (
                "0x3f699a30c273a1b39e1346dd63dfafa384977f94",
                "NZTA3PJBp9zYyj32Cozheuxqo7S1yqC9Vj",
            ),
        ];
        for (script_hash, address) in vectors {
            let script_hash = UInt160::parse(script_hash).unwrap();
            assert_eq!(script_hash.to_address(), address);
            assert_eq!(
                UInt160::from_address(address).unwrap().to_hex_string(),
                script_hash.to_hex_string()
            );
        }
    }

    #[test]
    fn from_address_rejects_wrong_version_before_checksum() {
        let mut payload = Vec::with_capacity(1 + UINT160_SIZE);
        payload.push(crate::constants::ADDRESS_VERSION.wrapping_add(1));
        payload.extend_from_slice(&[0x11; UINT160_SIZE]);
        let address = bs58::encode(payload).with_check().into_string();
        let err = UInt160::from_address(&address).unwrap_err().to_string();
        assert!(err.contains("Invalid address version"), "{err}");
    }

    #[test]
    fn from_address_rejects_missing_base58_check_checksum() {
        let err = UInt160::from_address("1").unwrap_err().to_string();
        assert!(err.contains("Invalid address length"), "{err}");
    }

    #[test]
    fn from_address_rejects_invalid_base58_check_checksum() {
        let script_hash = UInt160::from_bytes(&[0x24; UINT160_SIZE]).unwrap();
        let mut address = script_hash.to_address().into_bytes();
        let last = address.last_mut().unwrap();
        *last = if *last == b'1' { b'2' } else { b'1' };
        let address = String::from_utf8(address).unwrap();
        let err = UInt160::from_address(&address).unwrap_err().to_string();
        assert!(err.contains("Invalid address checksum"), "{err}");
    }

    proptest! {
        #[test]
        fn test_roundtrip_from_bytes(bytes in any::<[u8; UINT160_SIZE]>()) {
            let uint = UInt160::from_bytes(&bytes).unwrap();
            prop_assert_eq!(bytes, uint.to_array());
        }

        #[test]
        fn test_parse_hex_string(hex in "[0-9a-fA-F]{40}") {
            let uint = UInt160::parse(&format!("0x{}", hex)).unwrap();
            let uint2 = UInt160::parse(&uint.to_hex_string()).unwrap();
            prop_assert_eq!(uint, uint2);
        }

        #[test]
        fn test_ordering_transitive(
            a in any::<[u8; UINT160_SIZE]>(),
            b in any::<[u8; UINT160_SIZE]>(),
            c in any::<[u8; UINT160_SIZE]>()
        ) {
            let a = UInt160::from_bytes(&a).unwrap();
            let b = UInt160::from_bytes(&b).unwrap();
            let c = UInt160::from_bytes(&c).unwrap();
            if a < b && b < c { prop_assert!(a < c); }
            if a > b && b > c { prop_assert!(a > c); }
        }

        #[test]
        fn test_is_zero_correct(bytes in any::<[u8; UINT160_SIZE]>()) {
            let uint = UInt160::from_bytes(&bytes).unwrap();
            prop_assert_eq!(uint.is_zero(), bytes.iter().all(|&b| b == 0));
        }

        #[test]
        fn test_to_array_roundtrip(bytes in any::<[u8; UINT160_SIZE]>()) {
            let uint = UInt160::from_bytes(&bytes).unwrap();
            prop_assert_eq!(bytes, uint.to_array());
        }

        #[test]
        fn test_get_hash_code_deterministic(bytes in any::<[u8; UINT160_SIZE]>()) {
            let uint = UInt160::from_bytes(&bytes).unwrap();
            prop_assert_eq!(uint.hash_code(), uint.hash_code());
        }

        #[test]
        fn test_equals_is_symmetric(
            a in any::<[u8; UINT160_SIZE]>(),
            b in any::<[u8; UINT160_SIZE]>()
        ) {
            let uint_a = UInt160::from_bytes(&a).unwrap();
            let uint_b = UInt160::from_bytes(&b).unwrap();
            prop_assert_eq!(uint_a.equals(Some(&uint_b)), uint_b.equals(Some(&uint_a)));
        }

        #[test]
        fn test_from_address_roundtrip(address in "[1-9A-HJ-NP-Za-km-z]{34,34}") {
            let uint = UInt160::from_address(&address);
            if let Ok(parsed) = uint {
                prop_assert_eq!(address, parsed.to_address());
            }
        }
    }
}
