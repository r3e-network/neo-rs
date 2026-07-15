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
//! - `codec`: shared StackItem encode/decode helpers.
//! - `committee`: committee calculation helpers.
//! - `engine`: ApplicationEngine prelude helpers (persisting block).
//! - `invoke`: native ABI method binding helpers.
//! - `keys`: native-contract storage key helpers.
//! - `macros`: uniform native-contract declarations and dispatch generation.
//! - `settings`: shared storage-setting read/write helpers.
//! - `token`: shared NEP descriptors, account codecs, and storage encoding.

#[macro_use]
mod macros;
pub(crate) mod args;
pub(crate) mod codec;
pub(crate) mod committee;
pub(crate) mod engine;
pub(crate) mod invoke;
pub(crate) mod keys;
pub(crate) mod settings;
pub(crate) mod token;
