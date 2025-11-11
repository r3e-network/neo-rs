#[derive(Debug, Clone, thiserror::Error)]
pub enum SignError {
    #[error("ecdsa: invalid private key")]
    InvalidKey,

    #[error("ecdsa: signing failed")]
    SigningFailed,
}

#[derive(Debug, Clone, thiserror::Error)]
pub enum VerifyError {
    #[error("ecdsa: invalid public key")]
    InvalidKey,

    #[error("ecdsa: invalid signature")]
    InvalidSignature,
}
