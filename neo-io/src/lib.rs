//! # neo-io
//!
//! Deterministic binary IO primitives and serialization traits for Neo data.
//!
//! This crate is a Neo protocol facade over standard Rust and vetted ecosystem
//! IO building blocks. It should delegate generic mechanics to `std::io`,
//! `bytes`, `lz4_flex`, and similar crates, while keeping only Neo-specific
//! rules here: compact var-int encoding, C#-compatible reader/writer method
//! names, protocol length checks, and deterministic error mapping.
//!
//! ## Boundary
//!
//! This codec crate owns byte-level IO contracts and must not decide protocol
//! policy, storage layout, or node orchestration. Do not introduce custom
//! compression, buffering, endian, or stream abstractions unless an existing
//! library cannot preserve Neo wire compatibility.
//!
//! ## Contents
//!
//! - `codec`: Deterministic byte codecs and compression helpers used by Neo
//!   wire data.
//! - `core`: Core reader, writer, var-int, and macro helpers for binary IO.
//! - `serializable`: Serializable traits and compatibility helpers for Neo
//!   binary data.

mod codec;
pub use codec::compression;

#[macro_use]
mod core;
pub use core::macros;
pub mod serializable;
pub use core::var_int;

pub use core::{BinaryWriter, IoError, IoResult, MemoryReader, OptionExt, ValidateLength};
pub use serializable::Serializable;
pub use serializable::SerializableExtensions;

// Re-export compression types
pub use compression::{COMPRESSION_MIN_SIZE, COMPRESSION_THRESHOLD, Lz4};
