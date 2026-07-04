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
//! - `codec`: shared StackValue encode/decode helpers.
//! - `committee`: committee calculation helpers.
//! - `engine`: ApplicationEngine prelude helpers (persisting block).
//! - `keys`: native-contract storage key helpers.
//! - `settings`: shared storage-setting read/write helpers.

pub(crate) mod args;
pub(crate) mod codec;
pub(crate) mod committee;
pub(crate) mod engine;
pub(crate) mod keys;
pub(crate) mod settings;
