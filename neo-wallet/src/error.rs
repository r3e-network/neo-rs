use alloc::string::String;

use thiserror::Error;

#[derive(Debug, Error, PartialEq, Eq)]
pub enum WalletError {
    #[error("account not found")]
    AccountNotFound,

    #[error("duplicate account")]
    DuplicateAccount,

    #[error("keystore passphrase required")]
    PassphraseRequired,

    #[error("invalid keystore payload")]
    InvalidKeystore,

    #[error("crypto error: {0}")]
    Crypto(&'static str),

    #[error("keystore integrity mismatch")]
    IntegrityMismatch,

    #[error("wallet storage error: {0}")]
    Storage(String),
}
