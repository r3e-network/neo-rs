//! HSM Signer abstraction

mod hsm_signer;

pub(crate) use hsm_signer::{normalize_public_key, script_hash_from_public_key};
pub use hsm_signer::{HsmKeyInfo, HsmSigner};
