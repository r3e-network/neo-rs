//! HSM Signer abstraction

mod hsm_signer;

pub use hsm_signer::{
    HsmKeyInfo, HsmSigner, normalize_public_key, script_hash_from_public_key,
    signature_redeem_script,
};
