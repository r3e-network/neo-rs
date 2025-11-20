//! SQLite wallet plugin (stub)
//!
//! The C# node supports SQLite-based wallets (`.db3`). The full implementation
//! has not been ported yet, but we expose a stubbed wallet and plugin so builds
//! that enable the `sqlite-wallet` feature continue to compile cleanly and fail
//! with clear messaging.

pub mod plugin;
pub mod sq_lite_wallet;
pub mod sq_lite_wallet_factory;

pub use plugin::SqliteWalletPlugin;
pub use sq_lite_wallet::SQLiteWallet;
pub use sq_lite_wallet_factory::SQLiteWalletFactory;
