//! Helper utilities matching `Neo.Network.P2P.Helper`.
//!
//! The original C# helpers provide extension methods for the `Verifiable`
//! interface.  They are heavily used throughout the networking stack when
//! signing payloads or inspecting hashes.  This module mirrors that behaviour so
//! ported components can rely on identical semantics.

use neo_primitives::HASH_SIZE;

/// Helper utilities for working with `Verifiable` payloads.
const SIGN_DATA_LENGTH: usize = std::mem::size_of::<u32>() + HASH_SIZE;

/// Produces the byte buffer used for signing (network magic + payload hash),
/// mirroring `Neo.Network.P2P.Helper`.
pub fn get_sign_data<V>(
    verifiable: &V,
    network: u32,
) -> neo_error::CoreResult<[u8; SIGN_DATA_LENGTH]>
where
    V: neo_primitives::Verifiable + ?Sized,
{
    let hash = verifiable.hash().map_err(|e| neo_error::CoreError::invalid_operation(e.to_string()))?;
    let mut buffer = [0u8; SIGN_DATA_LENGTH];
    buffer[..4].copy_from_slice(&network.to_le_bytes());
    buffer[4..].copy_from_slice(&hash.as_bytes());
    Ok(buffer)
}

/// Convenience wrapper returning the signing data as a `Vec<u8>`.
pub fn get_sign_data_vec<V>(verifiable: &V, network: u32) -> neo_error::CoreResult<Vec<u8>>
where
    V: neo_primitives::Verifiable + ?Sized,
{
    Ok(get_sign_data(verifiable, network)?.to_vec())
}
