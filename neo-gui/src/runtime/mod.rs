//! # neo-gui::runtime
//!
//! Runtime flags, execution context state, and VM-facing support types.
//!
//! ## Boundary
//!
//! This module belongs to `neo-gui`. This application crate owns UI composition
//! and must call lower service/RPC APIs instead of reimplementing protocol
//! logic.
//!
//! ## Contents
//!
//! - `node`: Daemon composition, CLI modes, and long-running node startup.
//! - `sync`: poison-tolerant GUI mutex access.

pub mod node;
pub(crate) mod sync;
