//! ABI-level NeoVM opcode semantics shared by host runtimes.
//!
//! These helpers intentionally operate on public ABI [`crate::StackValue`]
//! values. Full interpreter-only behavior that requires reference identities
//! remains in `interpreter`, while compiled runtimes can reuse these functions
//! instead of maintaining private opcode rule tables.

pub mod arithmetic;
pub mod collections;
pub mod comparison;
pub mod conversion;
pub(crate) mod numeric;
pub mod splice;
pub(crate) mod stack;
pub(crate) mod stack_shape;
