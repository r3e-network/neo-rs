//! Phase 3 S6b: the HSM wallet adapter sank into neo-hsm; this shim freezes
//! the historical `crate::hsm_wallet::*` path for assembly consumers.

pub use neo_hsm::HsmWallet;
