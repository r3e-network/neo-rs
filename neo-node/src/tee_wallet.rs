//! TEE wallet adapter re-export (assembly-layer shim).
//!
//! Phase 3 S6c: the adapter implementation sank into
//! `neo-tee::wallet_adapter`; this shim freezes the historical
//! `crate::tee_wallet::TeeWalletAdapter` path used by startup/services.

pub use neo_tee::wallet_adapter::TeeWalletAdapter;
