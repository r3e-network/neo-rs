//! # neo-runtime::support
//!
//! Runtime-wide measurement and service support utilities.
//!
//! ## Boundary
//!
//! This module owns protocol-neutral runtime helpers. It must not depend on
//! concrete node composition, storage engines, or transport implementations.
//!
//! ## Contents
//!
//! - `time`: monotonic elapsed-time conversion helpers.

pub mod time;
