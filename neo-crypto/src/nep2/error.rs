use neo_base::encoding::FromBase58CheckError;

#[derive(Debug, Clone, thiserror::Error)]
pub enum Nep2Error {
    #[error("nep2: invalid format")]
    InvalidFormat,

    #[error("nep2: invalid address hash")]
    InvalidAddressHash,

    #[error("nep2: invalid private key")]
    InvalidPrivateKey,

    #[error("nep2: base58 decode failed")]
    Base58,

    #[error("nep2: scrypt {0}")]
    Scrypt(#[from] crate::scrypt::ScryptDeriveError),

    #[error("nep2: aes-ecb {0}")]
    Aes(#[from] crate::aes::AesEcbError),
}

impl From<FromBase58CheckError> for Nep2Error {
    fn from(_value: FromBase58CheckError) -> Self {
        Nep2Error::Base58
    }
}
