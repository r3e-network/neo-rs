use core::convert::TryInto;

use secp256k1::Secp256k1;

use crate::hash_algorithm::HashAlgorithm;

use super::super::error::Secp256k1Error;
use super::super::prehash::derive_prehash;

pub fn sign(
    message: &[u8],
    private_key: &[u8],
    algorithm: HashAlgorithm,
) -> Result<[u8; 64], Secp256k1Error> {
    let key_bytes: [u8; 32] = private_key
        .try_into()
        .map_err(|_| Secp256k1Error::InvalidPrivateKeyLength)?;
    let secret = secp256k1::SecretKey::from_slice(&key_bytes)
        .map_err(|_| Secp256k1Error::InvalidPrivateKey)?;
    let prehash = derive_prehash(message, algorithm);
    let msg = secp256k1::Message::from_digest_slice(&prehash)
        .map_err(|_| Secp256k1Error::SigningFailed)?;
    let secp = Secp256k1::new();
    let mut signature = secp.sign_ecdsa(&msg, &secret);
    signature.normalize_s();
    Ok(signature.serialize_compact())
}

pub fn sign_recoverable(
    message: &[u8],
    private_key: &[u8],
    algorithm: HashAlgorithm,
) -> Result<([u8; 64], u8), Secp256k1Error> {
    let key_bytes: [u8; 32] = private_key
        .try_into()
        .map_err(|_| Secp256k1Error::InvalidPrivateKeyLength)?;
    let secret = secp256k1::SecretKey::from_slice(&key_bytes)
        .map_err(|_| Secp256k1Error::InvalidPrivateKey)?;
    let prehash = derive_prehash(message, algorithm);
    let msg = secp256k1::Message::from_digest_slice(&prehash)
        .map_err(|_| Secp256k1Error::SigningFailed)?;
    let secp = Secp256k1::new();
    let signature = secp.sign_ecdsa_recoverable(&msg, &secret);
    let (recovery_id, bytes) = signature.serialize_compact();
    Ok((bytes, recovery_id.to_i32() as u8))
}
