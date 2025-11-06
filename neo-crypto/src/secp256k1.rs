extern crate alloc;

use alloc::vec::Vec;
use core::convert::TryInto;

use secp256k1::{
    ecdsa::{RecoverableSignature, RecoveryId, Signature},
    Secp256k1,
};

use crate::hash_algorithm::HashAlgorithm;

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum Secp256k1Error {
    #[error("secp256k1: invalid private key length")]
    InvalidPrivateKeyLength,
    #[error("secp256k1: invalid private key material")]
    InvalidPrivateKey,
    #[error("secp256k1: invalid public key encoding")]
    InvalidPublicKey,
    #[error("secp256k1: invalid signature encoding")]
    InvalidSignature,
    #[error("secp256k1: signing failed")]
    SigningFailed,
    #[error("secp256k1: signature verification failed")]
    VerificationFailed,
}

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

pub fn verify(
    message: &[u8],
    signature_bytes: &[u8],
    public_key: &[u8],
    algorithm: HashAlgorithm,
) -> Result<(), Secp256k1Error> {
    let mut signature =
        Signature::from_compact(signature_bytes).map_err(|_| Secp256k1Error::InvalidSignature)?;
    let public = secp256k1::PublicKey::from_slice(public_key)
        .map_err(|_| Secp256k1Error::InvalidPublicKey)?;
    let prehash = derive_prehash(message, algorithm);
    let msg = secp256k1::Message::from_digest_slice(&prehash)
        .map_err(|_| Secp256k1Error::VerificationFailed)?;
    let secp = Secp256k1::verification_only();
    if secp.verify_ecdsa(&msg, &signature, &public).is_ok() {
        return Ok(());
    }
    signature.normalize_s();
    secp.verify_ecdsa(&msg, &signature, &public)
        .map_err(|_| Secp256k1Error::VerificationFailed)
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

pub fn recover_public_key(
    message: &[u8],
    signature: &[u8; 64],
    recovery: u8,
    algorithm: HashAlgorithm,
) -> Result<Vec<u8>, Secp256k1Error> {
    let prehash = derive_prehash(message, algorithm);
    let msg = secp256k1::Message::from_digest_slice(&prehash)
        .map_err(|_| Secp256k1Error::VerificationFailed)?;
    let rec_id =
        RecoveryId::from_i32(recovery as i32).map_err(|_| Secp256k1Error::VerificationFailed)?;
    let recoverable = RecoverableSignature::from_compact(signature, rec_id)
        .map_err(|_| Secp256k1Error::InvalidSignature)?;
    let secp = Secp256k1::verification_only();
    let public = secp
        .recover_ecdsa(&msg, &recoverable)
        .map_err(|_| Secp256k1Error::VerificationFailed)?;
    Ok(public.serialize().to_vec())
}

fn derive_prehash(message: &[u8], algorithm: HashAlgorithm) -> [u8; 32] {
    let digest = algorithm.digest(message);
    let mut arr = [0u8; 32];
    if digest.len() >= 32 {
        arr.copy_from_slice(&digest[..32]);
    } else {
        let offset = 32 - digest.len();
        arr[offset..].copy_from_slice(&digest);
    }
    arr
}

#[cfg(test)]
mod tests {
    use super::*;
    use hex_literal::hex;

    #[test]
    fn sign_and_verify_roundtrip() {
        let private = hex!("1b7f730fc3ac386a1ae1c2cbaabdd99e3bb85da7d5236f9b1a92bb0b742d30ca");
        let secp = Secp256k1::new();
        let secret = secp256k1::SecretKey::from_slice(&private).unwrap();
        let public = secp256k1::PublicKey::from_secret_key(&secp, &secret).serialize();
        let signature = sign(b"neo-k1", &private, HashAlgorithm::Sha256).unwrap();
        verify(b"neo-k1", &signature, &public, HashAlgorithm::Sha256).unwrap();
    }

    #[test]
    fn supports_keccak_hashing() {
        let private = hex!("38b28fe5602eb700f8502e2c166b03db6602bc917e35a31995f1b2d287f4d137");
        let secp = Secp256k1::new();
        let secret = secp256k1::SecretKey::from_slice(&private).unwrap();
        let public = secp256k1::PublicKey::from_secret_key(&secp, &secret).serialize();
        let signature = sign(b"neo-keccak", &private, HashAlgorithm::Keccak256).unwrap();
        verify(b"neo-keccak", &signature, &public, HashAlgorithm::Keccak256).unwrap();
    }

    #[test]
    fn recover_public_key_from_recoverable_signature() {
        let private = hex!("6d16ca2b9f10f8917ac12f90b91f864b0db1d0545d142e9d5b75f1c83c5f4321");
        let secp = Secp256k1::new();
        let secret = secp256k1::SecretKey::from_slice(&private).unwrap();
        let expected = secp256k1::PublicKey::from_secret_key(&secp, &secret).serialize();
        let (sig, rec) =
            sign_recoverable(b"recover", &private, HashAlgorithm::Sha256).expect("sign");
        let recovered =
            recover_public_key(b"recover", &sig, rec, HashAlgorithm::Sha256).expect("recover");
        assert_eq!(expected.to_vec(), recovered);
    }
}
