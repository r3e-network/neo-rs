//! Helper utilities matching `Neo.Network.P2P.Helper`.
//!
//! The original C# helpers provide extension methods for the `IVerifiable`
//! interface.  They are heavily used throughout the networking stack when
//! signing payloads or inspecting hashes.  This module mirrors that behaviour so
//! ported components can rely on identical semantics.

use crate::neo_config::HASH_SIZE;
use crate::{error::CoreResult, IVerifiable, UInt256};

/// Helper utilities for working with `IVerifiable` payloads.
#[derive(Debug, Clone, Copy, Default)]
pub struct Helper;

impl Helper {
    const SIGN_DATA_LENGTH: usize = std::mem::size_of::<u32>() + HASH_SIZE;

    /// Computes the hash for a verifiable entity (mirrors `CalculateHash`).
    pub fn calculate_hash<V>(verifiable: &V) -> CoreResult<UInt256>
    where
        V: IVerifiable + ?Sized,
    {
        verifiable.hash()
    }

    /// Attempts to retrieve the verifiable hash, returning `None` on failure.
    pub fn try_get_hash<V>(verifiable: &V) -> Option<UInt256>
    where
        V: IVerifiable + ?Sized,
    {
        verifiable.hash().ok()
    }

    /// Produces the byte buffer used for signing (network magic + payload hash).
    pub fn get_sign_data<V>(
        verifiable: &V,
        network: u32,
    ) -> CoreResult<[u8; Self::SIGN_DATA_LENGTH]>
    where
        V: IVerifiable + ?Sized,
    {
        let hash = verifiable.hash()?;
        let mut buffer = [0u8; Self::SIGN_DATA_LENGTH];
        buffer[..4].copy_from_slice(&network.to_le_bytes());
        buffer[4..].copy_from_slice(&hash.as_bytes());
        Ok(buffer)
    }

    /// Convenience wrapper that returns the signing data as a `Vec<u8>`.
    pub fn get_sign_data_vec<V>(verifiable: &V, network: u32) -> CoreResult<Vec<u8>>
    where
        V: IVerifiable + ?Sized,
    {
        Ok(Self::get_sign_data(verifiable, network)?.to_vec())
    }
}
