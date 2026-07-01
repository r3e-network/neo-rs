//! # neo-blockchain::pipeline
//!
//! Ordered validation, execution, native-hook, and persistence steps for block
//! import.
//!
//! ## Boundary
//!
//! This module belongs to `neo-blockchain`. This node-service crate owns the
//! concrete block-import path and must not depend upward on composition, RPC,
//! GUI, or binaries.
//!
//! ## Contents
//!
//! - `block_processing`: block execution and persistence workflow.
//! - `block_validation`: block validation workflow.
//! - `handlers`: service message handlers.
//! - `native_persist`: native-contract persistence hooks.

pub mod block_processing;
pub mod block_validation;
pub mod handlers;
pub mod native_persist;
