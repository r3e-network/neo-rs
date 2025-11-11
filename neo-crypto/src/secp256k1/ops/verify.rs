use secp256k1::ecdsa::Signature;

use crate::hash_algorithm::HashAlgorithm;

use super::super::error::Secp256k1Error;
use super::super::prehash::derive_prehash;

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
    let secp = secp256k1::Secp256k1::verification_only();
    if secp.verify_ecdsa(&msg, &signature, &public).is_ok() {
        return Ok(());
    }
    signature.normalize_s();
    secp.verify_ecdsa(&msg, &signature, &public)
        .map_err(|_| Secp256k1Error::VerificationFailed)
}
