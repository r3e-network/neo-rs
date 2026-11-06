//! Extension helpers mirroring the C# `Neo.Extensions` namespace.
//!
//! Only generic, dependency-free extensions live here. Extensions that require
//! `neo-io` (serialisation, LZ4 compression) remain in `neo-core`.

/// General-purpose extension utilities (global log level, handler).
pub mod utility;

pub use utility::ExtensionsUtility;
