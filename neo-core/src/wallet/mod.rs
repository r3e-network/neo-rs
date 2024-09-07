// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use crate::types::{H160, Network};

mod asset_descriptor;
mod helper;
mod iwallet_factory;
mod iwallet_provider;
mod key_pair;
mod transfer_output;
mod wallet;
mod wallet_account;

pub use asset_descriptor::AssetDescriptor;
pub use iwallet_factory::IWalletFactory;
pub use iwallet_provider::IWalletProvider;
pub use key_pair::KeyPair;
pub use transfer_output::TransferOutput;
pub use wallet_account::WalletAccount;

pub mod nep2;
pub mod nep6;
mod nep6;

pub type Account = nep6::Account;

pub trait AccountHolder {
    fn get_account(&self, script_hash: &H160) -> Option<Account>;
}


pub trait Wallet: AccountHolder {
    type CreateError;

    fn network(&self) -> Network;

    fn create_account(&mut self, name: &str, passphrase: &[u8]) -> Result<&Account, Self::CreateError>;

    fn delete_account(&self, script_hash: &H160) -> bool;

    fn change_password(&mut self, old: &[u8], new: &[u8]) -> bool;
}