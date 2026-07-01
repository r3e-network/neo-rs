//! # neo-system::errors
//!
//! Typed errors and result aliases for this crate boundary.
//!
//! ## Boundary
//!
//! This module belongs to `neo-system`. This composition crate wires services
//! and must not hide protocol rules or duplicate lower-layer business logic.
//!
//! ## Contents
//!
//! - `error`: Typed error definitions and conversions.

pub mod error;

pub use error::{NodeError, NodeResult};
