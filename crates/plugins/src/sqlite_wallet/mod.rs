//! SQLite wallet module
//!
//! This module wires together the SQLite wallet components ported from the
//! C# implementation. The detailed implementations live in the sibling files.

pub mod account;
pub mod address;
pub mod contract;
pub mod key;
pub mod sq_lite_wallet;
pub mod sq_lite_wallet_account;
pub mod sq_lite_wallet_factory;
pub mod verification_contract;
pub mod wallet_data_context;

pub use sq_lite_wallet::SQLiteWallet;
pub use sq_lite_wallet_account::SQLiteWalletAccount;
pub use sq_lite_wallet_factory::SQLiteWalletFactory;
