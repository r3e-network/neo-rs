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
