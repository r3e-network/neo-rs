//! # neo-io::codec
//!
//! Deterministic byte codecs and compression helpers used by Neo wire data.
//!
//! ## Boundary
//!
//! This module belongs to `neo-io`. This codec crate owns byte-level IO
//! contracts and must not decide protocol policy, storage layout, or node
//! orchestration.
//!
//! ## Contents
//!
//! - `compression`: Compression codecs and deterministic envelope helpers.

pub mod compression;
