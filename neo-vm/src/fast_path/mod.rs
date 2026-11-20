//! Fast path execution vocabulary for common transaction patterns.
//!
//! The VM owns only the semantics-free pieces: the fast-path error vocabulary
//! ([`FastPathError`]) and a generic bytecode cursor ([`InstructionCursor`]).
//! Application-level fast-path decisions - recognizing the NEP-17 transfer
//! template, classifying transactions, and direct native dispatch - live with
//! the application engine in `neo-smart-contract`.
//!
//! ## Safety
//!
//! A fast path must produce **exactly** the same results as the full VM. Any
//! script the cursor cannot decode strictly must fall back to the canonical
//! interpreter.

pub mod error;
pub mod script_cursor;

pub use error::{FastPathError, FastPathResult};
pub use script_cursor::{EntryInstruction, InstructionCursor};
