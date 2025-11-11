use alloc::{string::String, vec::Vec};

use neo_base::{hash::Hash160, AddressVersion};
use neo_crypto::ecc256::Keypair;

use crate::{
    error::WalletError,
    nep6::{Nep6Account, Nep6Scrypt},
    signer::SignerScopes,
};

use super::{
    contract::{contract_from_nep6, contract_to_nep6, Contract},
    signer_extra::{embed_signer_extra, parse_signer_extra},
};

mod account;
mod crypto;
mod metadata;
mod nep6;
#[cfg(test)]
mod tests;

pub use account::Account;
