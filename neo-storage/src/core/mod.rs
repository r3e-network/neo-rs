//! # neo-storage::core
//!
//! Core reader, writer, var-int, and macro helpers for binary IO.
//!
//! ## Boundary
//!
//! This module belongs to `neo-storage`. This infrastructure crate owns store
//! mechanics and must not execute contracts, import blocks, or make RPC/network
//! policy decisions.
//!
//! ## Contents
//!
//! - `hash_utils`: storage hash helper functions.
//! - `key_builder`: storage key builder helpers.

pub mod hash_utils;
pub mod key_builder;

pub use hash_utils::{DEFAULT_XX_HASH3_SEED, XxHash3};
pub use key_builder::{KeyBuilder, KeyBuilderError};
