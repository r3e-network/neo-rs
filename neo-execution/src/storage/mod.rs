//! # neo-execution::storage
//!
//! Storage contexts, key builders, and storage item helpers for execution.
//!
//! ## Boundary
//!
//! This module belongs to `neo-execution`. This execution crate owns VM/native
//! interop behavior and must not own durable storage engines, P2P sync, or
//! application startup.
//!
//! ## Contents
//!
//! - `key_builder`: storage key builder helpers.
//! - `storage_context`: storage context records.
//! - `storage_item_ext`: storage item extension helpers.

pub mod key_builder;
pub mod storage_context;
pub mod storage_item_ext;
