#![cfg_attr(not(feature = "std"), no_std)]

//! Wallet primitives for managing Neo N3 accounts.

extern crate alloc;

mod account;
mod error;
mod keystore;
mod nep6;
mod signer;
mod wallet;

pub use account::Account;
pub use error::WalletError;
pub use keystore::{Keystore, KeystoreEntry};
pub use nep6::{Nep6Account, Nep6Contract, Nep6Parameter, Nep6Scrypt, Nep6Wallet};
pub use signer::{Signer, SignerScopes};

#[cfg(feature = "std")]
pub use keystore::{load_keystore, persist_keystore};
#[cfg(feature = "std")]
pub use wallet::WalletStorage;
pub use wallet::{AccountDetails, Wallet};
