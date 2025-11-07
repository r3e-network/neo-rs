use alloc::string::String;

use neo_crypto::nep2::Nep2Error;
use thiserror::Error;

#[derive(Debug, Error)]
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

    #[error("nep2 error: {0}")]
    Nep2(#[from] Nep2Error),

    #[error("invalid NEP-6 payload: {0}")]
    InvalidNep6(&'static str),

    #[error("invalid address: {0}")]
    InvalidAddress(String),

    #[error("serialization error: {0}")]
    Serialization(String),

    #[error("account is locked")]
    AccountLocked,

    #[error("watch-only account cannot sign or export private material")]
    WatchOnly,

    #[error("invalid WIF: {0}")]
    InvalidWif(String),

    #[error("invalid signer metadata: {0}")]
    InvalidSignerMetadata(&'static str),
}
