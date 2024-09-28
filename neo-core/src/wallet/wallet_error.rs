use thiserror::Error;

#[derive(Error, Debug)]
pub enum WalletError {
    #[error("Invalid password")]
    InvalidPassword,

    #[error("Account not found")]
    AccountNotFound,

    #[error("Insufficient funds")]
    InsufficientFunds,

    #[error("Invalid address")]
    InvalidAddress,

    #[error("Failed to create account")]
    AccountCreationFailed,

    #[error("Failed to sign transaction")]
    SigningFailed,

    #[error("Failed to encrypt wallet")]
    EncryptionFailed,

    #[error("Failed to decrypt wallet")]
    DecryptionFailed,

    #[error("Invalid key format")]
    InvalidKeyFormat,

    #[error("Wallet file not found")]
    WalletFileNotFound,

    #[error("IO error: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Serialization error: {0}")]
    SerializationError(#[from] serde_json::Error),

    #[error("Failed to create key pair: {0}")]
    KeyPairCreationError(String),

    #[error("Failed to import from WIF: {0}")]
    WifImportError(String),

    #[error("Failed to import from NEP2: {0}")]
    Nep2ImportError(String),

    #[error("Invalid JSON in wallet file: {0}")]
    InvalidJson(#[from] serde_json::Error),

    #[error("Unsupported wallet version")]
    UnsupportedVersion,

    #[error("Failed to encrypt key: {0}")]
    EncryptionError(String),

    #[error("Failed to decrypt key: {0}")]
    DecryptionError(String),

    #[error("Invalid script hash")]
    InvalidScriptHash,
    
    #[error("Invalid operation: {0}")]
    InvalidOperation(String),

    #[error("Unknown error: {0}")]
    Unknown(String),
}

impl From<String> for WalletError {
    fn from(error: String) -> Self {
        WalletError::Unknown(error)
    }
}
