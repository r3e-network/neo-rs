#![cfg_attr(not(feature = "std"), no_std)]

//! Wallet primitives for managing Neo N3 accounts.

extern crate alloc;

mod account;
mod error;
mod keystore;
mod wallet;

pub use account::Account;
pub use error::WalletError;
pub use keystore::{Keystore, KeystoreEntry};

#[cfg(feature = "std")]
pub use keystore::{load_keystore, persist_keystore};
pub use wallet::Wallet;
#[cfg(feature = "std")]
pub use wallet::WalletStorage;
