//! # neo-native-contracts::registry
//!
//! Native contract registry and dispatch helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `catalog`: native contract catalog records.
//! - `hashes`: Hash functions and hash-domain helpers used by protocol code.
//! - `native_contract`: native contract trait and base behavior.
//! - `provider`: Provider adapter for the surrounding trait boundary.
//! - `role`: native role identifiers.

pub mod catalog;
pub mod hashes;
pub mod native_contract;
pub mod provider;
pub mod role;
