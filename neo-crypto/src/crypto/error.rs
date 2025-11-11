use crate::{ecdsa, secp256k1};

/// Errors returned by `sign`.
#[derive(Debug, thiserror::Error)]
pub enum SignError {
    #[error("invalid private key material")]
    InvalidPrivateKey,
    #[error(transparent)]
    Signing(#[from] ecdsa::SignError),
    #[error(transparent)]
    Secp256k1(#[from] secp256k1::Secp256k1Error),
}

/// Errors returned by `verify`.
#[derive(Debug, thiserror::Error)]
pub enum VerifyError {
    #[error("invalid public key encoding")]
    InvalidPublicKey,
    #[error("invalid signature payload")]
    InvalidSignature,
    #[error(transparent)]
    Signature(#[from] ecdsa::VerifyError),
    #[error(transparent)]
    Secp256k1(#[from] secp256k1::Secp256k1Error),
}
