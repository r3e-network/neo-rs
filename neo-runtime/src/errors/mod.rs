//! # neo-runtime::errors
//!
//! Typed errors and result aliases for this crate boundary.
//!
//! ## Boundary
//!
//! This module belongs to `neo-runtime`. This runtime API crate owns shared
//! service contracts and must not depend on concrete node binaries or UI
//! composition.
//!
//! ## Contents
//!
//! - `error`: Typed error definitions and conversions.

pub mod error;

pub use error::{ServiceError, ServiceResult};
