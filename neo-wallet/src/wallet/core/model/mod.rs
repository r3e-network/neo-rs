use crate::nep6::Nep6Wallet;

#[allow(unused_imports)]
pub(super) use super::details::AccountDetails;
#[allow(unused_imports)]
pub(super) use crate::{
    account::{self, Account, Contract},
    error::WalletError,
    keystore::{decrypt_entry, Keystore},
};
#[allow(unused_imports)]
pub(super) use alloc::{collections::BTreeMap, string::String, vec::Vec};
#[allow(unused_imports)]
pub(super) use neo_base::encoding::{WifDecode, WifEncode};
#[allow(unused_imports)]
pub(super) use neo_base::{hash::Hash160, AddressVersion};
#[allow(unused_imports)]
pub(super) use neo_crypto::ecc256::PrivateKey;
#[allow(unused_imports)]
pub(super) use neo_crypto::{nep2::encrypt_nep2, scrypt::ScryptParams, SignatureBytes};

mod crypto;
mod manage;
mod nep6;
mod wallet;

pub use wallet::Wallet;
