//! # neo-vm::execution_context
//!
//! NeoVM execution context frames and instruction-pointer state.
//!
//! ## Boundary
//!
//! This module belongs to `neo-vm`. This VM crate owns deterministic script
//! execution and must not own ledger persistence, network transport, or node
//! composition.
//!
//! ## Contents
//!
//! - `context`: Runtime context records carried through the local workflow.
//! - `shared_states`: shared execution-context state records.

pub mod context;
pub mod shared_states;
pub use context::{ExecutionContext, Slot};
pub use shared_states::SharedStates;
