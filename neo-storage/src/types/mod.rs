//! # neo-storage::types
//!
//! Storage-domain types shared by store implementations.
//!
//! ## Boundary
//!
//! This module belongs to `neo-storage`. This infrastructure crate owns store
//! mechanics and must not execute contracts, import blocks, or make RPC/network
//! policy decisions.
//!
//! ## Contents
//!
//! - `seek`: seek direction re-exports.
//! - `storage_item`: storage item records.
//! - `storage_key`: storage key records and encoders.
//! - `track`: tracked mutation state re-exports.

/// Storage iteration direction.
pub mod seek;
/// Value stored in contract storage.
pub mod storage_item;
/// Contract storage key representation.
pub mod storage_key;
/// Cache tracking state for a storage entry.
pub mod track;

pub use seek::SeekDirection;
pub use storage_item::StorageItem;
pub use storage_key::StorageKey;
pub use track::TrackState;
