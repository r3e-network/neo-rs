//! Public boundary for the offline verified MDBX rebase implementation.
//!
//! The implementation lives in a separate module so the operational entry
//! point stays within the repository's review-size budget.

#[path = "rebase_impl.rs"]
mod implementation;
#[path = "rebase_support.rs"]
mod support;

pub use implementation::{
    MDBX_REBASE_INCOMPLETE_FILE, MdbxExactKeyExclusion, MdbxRebaseOptions, MdbxRebaseReport,
    MdbxRebaseTableReport, finalize_mdbx_rebase, rebase_mdbx_environment,
};
