//! HSM Signer abstraction

mod hsm_signer;

pub use hsm_signer::{HsmKeyInfo, HsmSigner};
pub(crate) use hsm_signer::{normalize_public_key, script_hash_from_public_key};
