//! # neo-blockchain::messages
//!
//! Typed service commands, events, and payload wrappers for the crate boundary.
//!
//! ## Boundary
//!
//! This module belongs to `neo-blockchain`. This node-service crate owns the
//! concrete block-import path and must not depend upward on composition, RPC,
//! GUI, or binaries.
//!
//! ## Contents
//!
//! - `fill_completed`: fill-completed service event payload.
//! - `fill_memory_pool`: memory-pool fill command payload.
//! - `import`: block import command payload.
//! - `import_completed`: block import completion event payload.
//! - `inventory_payload`: inventory relay payload.
//! - `relay_result`: transaction relay verdict payload.
//! - `reverify`: transaction reverification command payload.

pub mod fill_completed;
pub mod fill_memory_pool;
pub mod import;
pub mod import_completed;
pub mod inventory_payload;
pub mod relay_result;
pub mod reverify;
