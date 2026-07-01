//! # neo-gui::client
//!
//! Client-side adapters for remote services and RPC access.
//!
//! ## Boundary
//!
//! This module belongs to `neo-gui`. This application crate owns UI composition
//! and must call lower service/RPC APIs instead of reimplementing protocol
//! logic.
//!
//! ## Contents
//!
//! - `rpc`: RPC client adapter for the GUI.

pub mod rpc;
