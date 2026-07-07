//! # neo-io::serializable
//!
//! Serializable traits and compatibility helpers for Neo binary data.
//!
//! ## Boundary
//!
//! This module belongs to `neo-io`. This codec crate owns byte-level IO
//! contracts and must not decide protocol policy, storage layout, or node
//! orchestration.
//!
//! ## Contents
//!
//! - `helper`: shared helper functions.
//! - `macros`: declarative helpers for implementing Neo binary serialization.
//! - `primitives`: Primitive serialization implementations.
//! - `traits`: Serializable traits and extension helpers.

pub mod helper;
mod macros;
pub mod primitives;
mod traits;

pub use traits::{Serializable, SerializableExtensions};
