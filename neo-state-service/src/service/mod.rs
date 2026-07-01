//! # neo-state-service::service
//!
//! Service loops, handles, lifecycle helpers, and command processing.
//!
//! ## Boundary
//!
//! This module belongs to `neo-state-service`. This service crate owns state-
//! root and MPT service behavior and must not own block download, consensus,
//! RPC transport, or UI composition.
//!
//! ## Contents
//!
//! - `commit_handlers`: state-service commit handlers.

pub mod commit_handlers;
