//! Console helpers shared across CLI command modules.
//!
//! This module mirrors the helper types defined under `Neo.CLI/CLI/*` so the
//! Rust CLI can gradually adopt the same abstractions as the C# implementation.

pub mod command_line_options;
pub mod helper;
pub mod parse_function_attribute;
pub mod percent;
