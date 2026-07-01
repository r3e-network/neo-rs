//! # neo-gui::shell
//!
//! GUI application shell, event loop, and top-level window composition.
//!
//! ## Boundary
//!
//! This module belongs to `neo-gui`. This application crate owns UI composition
//! and must call lower service/RPC APIs instead of reimplementing protocol
//! logic.
//!
//! ## Contents
//!
//! - `app`: GUI application shell.

pub mod app;
