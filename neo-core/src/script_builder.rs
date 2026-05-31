//! Script builder for the Neo Virtual Machine.
//!
//! The implementation now lives in the dedicated [`neo_script_builder`] crate
//! (C# `Neo.VM.ScriptBuilder`), layered on `neo-vm-rs` *below* `neo-core` so
//! crypto/payload layers can construct redeem scripts without depending on the
//! stateful VM host or the smart-contract engine. This module re-exports it so
//! the historical `neo_core::ScriptBuilder` / `neo_core::script_builder::ScriptBuilder`
//! paths stay stable for existing callers.
//!
//! `CoreError` provides `From<ScriptBuilderError>` (see `crate::error`) so
//! callers returning `CoreResult` keep using `?` on the fallible emitters
//! unchanged.

pub use neo_script_builder::{ScriptBuilder, ScriptBuilderError, ScriptBuilderResult};
