//! # neo-config::errors
//!
//! Typed errors and result aliases for this crate boundary.
//!
//! ## Boundary
//!
//! This module belongs to `neo-config`. This configuration crate owns typed
//! settings and must not open storage, start services, or run protocol
//! workflows.
//!
//! ## Contents
//!
//! - `error`: Typed error definitions and conversions.

pub mod error;

pub use error::{ConfigError, ConfigResult};
