//! # neo-data-cache
//!
//! Canonical home for `DataCache`, `StorageKey`, `StorageItem`, and the
//! persistence-cache facade used by `neo-core` and downstream crates.
//!
//! This crate is the **user-facing entry point** for the data-cache layer.
//! The low-level storage types and traits live in [`neo_storage`]; this crate
//! re-exports them and provides the historical `neo_core::persistence::*`
//! surface so existing call sites keep compiling unchanged.

#![doc(html_root_url = "https://docs.rs/neo-data-cache/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod cache;
pub mod storage_item;
pub mod storage_key;

// Re-exports of the canonical cache + storage types.
pub use cache::{
    DataCache, DataCacheConfig, DataCacheError, DataCacheResult, OnEntryDelegate, PrefetchPattern,
    Trackable, TrackableEntry,
};
pub use storage_item::StorageItem;
pub use storage_key::StorageKey;
