//! # neo-io::core
//!
//! Core reader, writer, var-int, and macro helpers for binary IO.
//!
//! ## Boundary
//!
//! This module belongs to `neo-io`. This codec crate owns byte-level IO
//! contracts and must not decide protocol policy, storage layout, or node
//! orchestration.
//!
//! ## Contents
//!
//! - `binary_writer`: Binary writer type and extension helpers.
//! - `macros`: Crate-local macros that keep protocol declarations compact.
//! - `memory_reader`: In-memory byte reader implementation.
//! - `var_int`: Neo variable-length integer codec.

pub mod binary_writer;

#[macro_use]
pub mod macros;

pub mod memory_reader;
pub mod var_int;

pub use binary_writer::BinaryWriter;
pub use macros::{OptionExt, ValidateLength};
pub use memory_reader::{IoError, IoResult, MemoryReader};
