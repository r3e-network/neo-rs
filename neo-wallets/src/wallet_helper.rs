//! Address / script-hash conversion helpers used by the wallet layer.
//!
//! The full wallet `Helper` (which includes the network-fee calculator,
//! transfer script construction, etc.) needs the application engine
//! and the native contracts; it lives in `neo-core::wallets_runtime`
//! for now. The pure-crypto address helpers extracted here are
//! what `KeyPair`, `WalletAccount`, and `Nep6Wallet` actually need.

use neo_error::{CoreError, CoreResult};
use neo_primitives::UInt160;

use neo_primitives::base58_check::{
    AddressDecodeError, Base58CheckDecodeError, decode_address_payload, encode_address_payload,
};

/// Convert a script hash + address version byte to a base58 address string.
pub fn to_address(script_hash: &UInt160, version: u8) -> String {
    encode_address_payload(version, &script_hash.to_array())
}

/// Convert a base58 address string back to a script hash, validating the
/// embedded version byte.
pub fn to_script_hash(address: &str, version: u8) -> CoreResult<UInt160> {
    let script_hash = decode_address_payload(address, version)
        .map_err(|err| CoreError::other(match err {
            AddressDecodeError::Base58(Base58CheckDecodeError::InvalidBase58 { message }) => {
                format!("Invalid Base58 string: {message}")
            }
            AddressDecodeError::Base58(Base58CheckDecodeError::MissingChecksum) => {
                "Invalid Base58Check format: decoded data length is too short (requires at least 4 checksum bytes).".to_string()
            }
            AddressDecodeError::Base58(Base58CheckDecodeError::InvalidChecksum) => {
                "Invalid Base58Check checksum: provided checksum does not match calculated checksum.".to_string()
            }
            AddressDecodeError::InvalidLength { actual, .. } => {
                format!("Invalid address format: expected 21 bytes after Base58Check decoding, but got {actual} bytes. The address may be corrupted or in an invalid format.")
            }
            AddressDecodeError::InvalidVersion { expected, actual } => {
                format!("Invalid address version: expected version {expected}, but got {actual}. The address may be for a different network.")
            }
        }))?;
    UInt160::from_bytes(&script_hash).map_err(|e| CoreError::other(format!("Invalid script hash bytes: {e}")))
}
