//! Helper utilities matching `Neo.Network.P2P.Helper`.
//!
//! The original C# helpers provide extension methods for the `Verifiable`
//! interface.  They are heavily used throughout the networking stack when
//! signing payloads or inspecting hashes.  This module mirrors that behaviour so
//! ported components can rely on identical semantics.

use neo_protocol::signing_data::SIGN_DATA_LENGTH;

/// Produces the byte buffer used for signing (network magic + payload hash),
/// mirroring `Neo.Network.P2P.Helper`.
pub fn get_sign_data<V>(
    verifiable: &V,
    network: u32,
) -> neo_smart_contract::error::CoreResult<[u8; SIGN_DATA_LENGTH]>
where
    V: crate::Verifiable + ?Sized,
{
    neo_protocol::signing_data::get_sign_data(verifiable, network)
        .map_err(|e| crate::error::CoreError::invalid_operation(e.to_string()))
}

/// Convenience wrapper returning the signing data as a `Vec<u8>`.
pub fn get_sign_data_vec<V>(
    verifiable: &V,
    network: u32,
) -> neo_smart_contract::error::CoreResult<Vec<u8>>
where
    V: crate::Verifiable + ?Sized,
{
    neo_protocol::signing_data::get_sign_data_vec(verifiable, network)
        .map_err(|e| crate::error::CoreError::invalid_operation(e.to_string()))
}
