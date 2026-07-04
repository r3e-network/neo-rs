//! # neo-execution::storage
//!
//! Storage contexts and storage item helpers for execution.
//!
//! ## Boundary
//!
//! This module belongs to `neo-execution`. This execution crate owns VM/native
//! interop behavior and must not own durable storage engines, P2P sync, or
//! application startup.
//!
//! ## Contents
//!
//! - `storage_context`: storage context records.
//! - `storage_item_ext`: storage item extension helpers.
//!
//! Note: The `KeyBuilder` newtype wrapper that previously lived here was removed
//! (ADR-022). Use `neo_storage::KeyBuilder` directly — it is the single source
//! of truth for storage key construction.

pub mod storage_context;
pub mod storage_item_ext;
