//! Helper utilities matching `Neo.Network.P2P.Helper`.
//!
//! The original C# helpers provide extension methods for the `IVerifiable`
//! interface.  They are heavily used throughout the networking stack when
//! signing payloads or inspecting hashes.  This module mirrors that behaviour so
//! ported components can rely on identical semantics.

use crate::neo_config::HASH_SIZE;

/// Helper utilities for working with `IVerifiable` payloads.
const SIGN_DATA_LENGTH: usize = std::mem::size_of::<u32>() + HASH_SIZE;

/// Produces the byte buffer used for signing (network magic + payload hash),
/// mirroring `Neo.Network.P2P.Helper`.
pub fn get_sign_data<V>(
    verifiable: &V,
    network: u32,
) -> crate::error::CoreResult<[u8; SIGN_DATA_LENGTH]>
where
    V: crate::IVerifiable + ?Sized,
{
    let hash = verifiable.hash()?;
    let mut buffer = [0u8; SIGN_DATA_LENGTH];
    buffer[..4].copy_from_slice(&network.to_le_bytes());
    buffer[4..].copy_from_slice(&hash.as_bytes());
    Ok(buffer)
}

/// Convenience wrapper returning the signing data as a `Vec<u8>`.
pub fn get_sign_data_vec<V>(verifiable: &V, network: u32) -> crate::error::CoreResult<Vec<u8>>
where
    V: crate::IVerifiable + ?Sized,
{
    Ok(get_sign_data(verifiable, network)?.to_vec())
}
