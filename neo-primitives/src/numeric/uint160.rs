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
        base58_check::Base58Check::encode_address_payload(
            crate::constants::ADDRESS_VERSION,
            &self.to_array(),
        )
    }

    /// Converts this `UInt160` to a Neo address string using an explicit
    /// address-version byte (matches C# `ToAddress(byte version)`).
    #[must_use]
    pub fn to_address_with_version(&self, version: u8) -> String {
        base58_check::Base58Check::encode_address_payload(version, &self.to_array())
    }

    /// Parses a Neo address string to a `UInt160`.
    ///
    /// # Errors
    ///
    /// Returns `PrimitiveError::InvalidFormat` if the address is not valid Base58,
    /// has an incorrect length, has an invalid version byte, or has an invalid checksum.
    pub fn from_address(address: &str) -> crate::PrimitiveResult<Self> {
        let script_hash = base58_check::Base58Check::decode_address_payload(
            address,
            crate::constants::ADDRESS_VERSION,
        )
        .map_err(map_base58_check_address_error)?;
        Self::from_bytes(&script_hash)
    }

    #[must_use]
    /// Returns the Neo-compatible 32-bit hash code for this value.
    // Rationale: Neo's C# hash-code algorithm intentionally folds unsigned
    // 64-bit words into signed 32-bit values with wrapping casts.
    #[allow(clippy::cast_possible_truncation)]
    #[allow(clippy::cast_possible_wrap)]
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
#[path = "../tests/numeric/uint160.rs"]
mod tests;
