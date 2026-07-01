//! # neo-config::network
//!
//! Network selection, genesis data, and chain identity configuration.
//!
//! ## Boundary
//!
//! This module belongs to `neo-config`. This configuration crate owns typed
//! settings and must not open storage, start services, or run protocol
//! workflows.
//!
//! ## Contents
//!
//! - `genesis`: genesis block and committee configuration.
//! - `network_type`: Neo network identifiers.

pub mod genesis;
pub mod network_type;
