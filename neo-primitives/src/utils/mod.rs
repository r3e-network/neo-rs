//! # neo-primitives::utils
//!
//! Small utility helpers shared within the crate.
//!
//! ## Boundary
//!
//! This module belongs to `neo-primitives`. This foundation crate must stay
//! free of node-service, storage-backend, RPC, and network orchestration
//! dependencies.
//!
//! ## Contents
//!
//! - `constants`: shared primitive constants.
//! - `time`: time conversion helpers.

pub mod constants;
pub mod time;
