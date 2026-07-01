//! # neo-gui::ui
//!
//! Reusable GUI theme and widget helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-gui`. This application crate owns UI composition
//! and must call lower service/RPC APIs instead of reimplementing protocol
//! logic.
//!
//! ## Contents
//!
//! - `theme`: GUI theme tokens and styling helpers.
//! - `widgets`: reusable GUI widgets.

pub mod theme;
pub mod widgets;
