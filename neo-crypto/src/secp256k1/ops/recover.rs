use alloc::vec::Vec;

use secp256k1::{
    ecdsa::{RecoverableSignature, RecoveryId},
    Secp256k1,
};

use crate::hash_algorithm::HashAlgorithm;

use super::super::error::Secp256k1Error;
use super::super::prehash::derive_prehash;

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
