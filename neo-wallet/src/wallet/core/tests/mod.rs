use super::Wallet;
use crate::{account::Account, signer::SignerScopes, WalletError};
use hex_literal::hex;
use neo_base::{hash::Hash160, AddressVersion};
use neo_crypto::{ecc256::PrivateKey, scrypt::ScryptParams};

mod basic;
mod import_export;
mod watch;

pub(super) const NEP2_VECTOR: &str = "6PYRzCDe46gkaR1E9AX3GyhLgQehypFvLG2KknbYjeNHQ3MZR2iqg8mcN3";
pub(super) const NEP2_PASSWORD: &str = "Satoshi";
pub(super) const WIF_VECTOR: &str = "L3tgppXLgdaeqSGSFw1Go3skBiy8vQAM7YMXvTHsKQtE16PBncSU";
