//! # neo-primitives::macros
//!
//! Crate-local macros that keep protocol declarations compact.
//!
//! ## Boundary
//!
//! This module belongs to `neo-primitives`. This foundation crate must stay
//! free of node-service, storage-backend, RPC, and network orchestration
//! dependencies.
//!
//! ## Contents
//!
//! - `display_hex`: hex display macro support.
//! - `message_flags`: P2P message flag records.
//! - `protocol_enum`: protocol enum macro support.
//! - `uint`: fixed-width integer macro support.

mod display_hex;
mod message_flags;
mod protocol_enum;
mod uint;
