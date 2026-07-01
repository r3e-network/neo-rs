//! # neo-io::extensions
//!
//! Extension traits layered over the core IO primitives.
//!
//! ## Boundary
//!
//! This module belongs to `neo-io`. This codec crate owns byte-level IO
//! contracts and must not decide protocol policy, storage layout, or node
//! orchestration.
//!
//! ## Contents
//!
//! - `binary_reader`: Binary reader extension trait and implementations.
//! - `binary_writer`: Binary writer type and extension helpers.
//! - `memory_reader`: In-memory byte reader implementation.
//! - `serializable`: Serializable traits and compatibility helpers for Neo
//!   binary data.

/// Binary-reader extension helpers.
pub mod binary_reader;
/// Binary-writer extension helpers.
pub mod binary_writer;
/// Memory-reader extension helpers.
pub mod memory_reader;
/// Serializable value and collection extension helpers.
pub mod serializable;
