//! # neo-vm::runtime
//!
//! Runtime flags, execution context state, and VM-facing support types.
//!
//! ## Boundary
//!
//! This module belongs to `neo-vm`. This VM crate owns deterministic script
//! execution and must not own ledger persistence, network transport, or node
//! composition.
//!
//! ## Contents
//!
//! - `evaluation_stack`: VM evaluation stack implementation.
//! - `interop_service`: VM interop service registry.
//! - `interoperable`: VM interoperability trait helpers.
//! - `reference_counter`: VM reference-counter implementation.
//! - `slot`: VM slot records and helpers.

pub mod evaluation_stack;
pub mod interop_service;
pub mod interoperable;
pub mod reference_counter;
pub mod slot;

pub use evaluation_stack::EvaluationStack;
pub use interop_service::InteropService;
pub use interoperable::{Interoperable, InteroperableError};
pub use reference_counter::{CompoundId, ReferenceCounter};
pub use slot::Slot;
