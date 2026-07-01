//! # neo-native-contracts::support
//!
//! Shared support helpers that keep domain modules focused.
//!
//! ## Boundary
//!
//! This module belongs to `neo-native-contracts`. This execution-domain crate
//! owns native contract logic and storage codecs and must not own node startup,
//! RPC transport, or P2P sync.
//!
//! ## Contents
//!
//! - `args`: native interop argument parsing.
//! - `committee`: committee calculation helpers.
//! - `keys`: native-contract storage key helpers.

pub(crate) mod args;
pub(crate) mod committee;
pub(crate) mod keys;
