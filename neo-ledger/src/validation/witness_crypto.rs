//! Signature and multi-signature witness cryptography.
//!
//! The implementation is owned by `neo-smart-contract` (the crate that owns the
//! crypto/VM-aware witness helpers). This module only re-exports it so ledger
//! validation keeps its historical paths and re-export surface.
//!
//! Callers must pass public keys in redeem-script order, exactly as
//! [`neo_smart_contract::helper::Helper::try_multi_sig_redeem_script`] builds the
//! script. Verification walks signature and key slots in that same order; there is
//! no implicit re-sorting, because C# `ECPoint` ordering is not lexicographic
//! compressed-key ordering.

pub use neo_smart_contract::witness_verification::{
    verify_witness_multi_signature, verify_witness_signature,
};
