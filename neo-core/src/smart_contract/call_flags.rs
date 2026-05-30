//! Permission flags for contract invocations.
//!
//! `CallFlags` is defined in `neo-primitives` (Layer 0) so the VM host and the
//! smart-contract layer can both depend on it without a cycle. Re-exported here
//! for the historical `neo_core::smart_contract::CallFlags` path.

pub use neo_primitives::CallFlags;
