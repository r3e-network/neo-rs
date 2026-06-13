//! Script builder for the Neo Virtual Machine.
//!
//! Re-exports [`ScriptBuilder`] from `neo-vm-rs` (the common VM crate) and
//! adds the `redeem_script` helpers that depend on `neo-crypto`.

pub mod redeem_script;

// Re-export the core ScriptBuilder from neo-vm-rs
pub use neo_vm_rs::script_builder::{
    ScriptBuilder, ScriptBuilderError, ScriptBuilderResult,
};

// Re-export redeem_script helpers (these depend on neo-crypto, so they stay in neo-vm)
pub use redeem_script::{
    RedeemScriptError, check_multisig_hash, check_sig_hash, is_multi_sig_contract,
    is_signature_contract, multi_sig_redeem_script_from_keys, multi_sig_redeem_script_from_points,
    parse_multi_sig_contract, parse_multi_sig_invocation, signature_redeem_script,
};
