//! TEE-protected wallet implementation
//!
//! Provides secure wallet storage using TEE sealing.

mod provider;
mod sealed_key;
mod tee_wallet;

pub use provider::TeeWalletProvider;
pub use sealed_key::SealedKey;
pub use tee_wallet::TeeWallet;
