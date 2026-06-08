//! `CallFlags` re-export from neo-primitives.
//!
//! `CallFlags` matches C# `Neo.SmartContract.CallFlags`: the permission set
//! granted when one contract invokes another. The canonical enum lives in
//! `neo-primitives`; this module re-exports it.

pub use neo_primitives::CallFlags;
