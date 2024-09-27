use thiserror::Error;

#[derive(Error, Debug)]
pub enum Nep6Error {
    #[error("Account not found")]
    AccountNotFound,

    #[error("Invalid password")]
    InvalidPassword,

    #[error("Failed to create key pair: {0}")]
    KeyPairCreationError(String),

    #[error("Failed to import from WIF: {0}")]
    WifImportError(String),

    #[error("Failed to import from NEP2: {0}")]
    Nep2ImportError(String),

    #[error("Failed to save wallet: {0}")]
    SaveError(#[from] std::io::Error),

    #[error("Invalid JSON in wallet file: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("Wallet file not found")]
    WalletFileNotFound,

    #[error("Unsupported wallet version")]
    UnsupportedVersion,

    #[error("Failed to encrypt key: {0}")]
    EncryptionError(String),

    #[error("Failed to decrypt key: {0}")]
    DecryptionError(String),

    #[error("Invalid script hash")]
    InvalidScriptHash,

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<String> for Nep6Error {
    fn from(error: String) -> Self {
        Nep6Error::Unknown(error)
    }
}
