//! Signing preimages for primitive-verifiable protocol payloads.

use neo_primitives::{PrimitiveResult, UINT256_SIZE, Verifiable};

pub const SIGN_DATA_LENGTH: usize = std::mem::size_of::<u32>() + UINT256_SIZE;

pub fn get_sign_data<V>(verifiable: &V, network: u32) -> PrimitiveResult<[u8; SIGN_DATA_LENGTH]>
where
    V: Verifiable + ?Sized,
{
    let hash = verifiable.hash()?;
    let mut buffer = [0u8; SIGN_DATA_LENGTH];
    buffer[..4].copy_from_slice(&network.to_le_bytes());
    buffer[4..].copy_from_slice(&hash.as_bytes());
    Ok(buffer)
}

pub fn get_sign_data_vec<V>(verifiable: &V, network: u32) -> PrimitiveResult<Vec<u8>>
where
    V: Verifiable + ?Sized,
{
    Ok(get_sign_data(verifiable, network)?.to_vec())
}
