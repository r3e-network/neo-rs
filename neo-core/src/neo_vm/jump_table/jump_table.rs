//! Jump table shim matching the C# layout.
//!
//! The actual implementation resides in `jump_table/mod.rs`; this module re-exports it
//! so that the file structure mirrors the C# project.

pub use super::JumpTable;
