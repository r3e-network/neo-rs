//! # neo-vm::jump_table::operations
//!
//! Stateless value, splice, numeric, and type opcode handlers.
//!
//! ## Boundary
//!
//! These modules implement deterministic NeoVM stack/value operations. Control
//! flow, slot access, and jump-table construction remain in the parent module.
//!
//! ## Contents
//!
//! - `bitwisee`: bitwise and equality handlers.
//! - `numeric`: arithmetic, comparison, and shift handlers.
//! - `splice`: byte-string and buffer splice handlers.
//! - `types`: runtime type checks and conversions.

pub mod bitwisee;
pub mod numeric;
pub mod splice;
pub mod types;

pub(super) use super::get_integer;
