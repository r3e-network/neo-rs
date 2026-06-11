//! Cache layer for the persistence facade.
//!
//! This module re-exports the canonical cache implementation from
//! [`neo_storage`]. The actual `DataCache` / `Trackable` / prefetch code lives
//! in `neo_storage::persistence::data_cache`; this module is the public
//! entry point of the `neo-data-cache` crate.

pub use neo_storage::persistence::data_cache::cache::*;
pub use neo_storage::persistence::data_cache::trackable::*;
pub use neo_storage::persistence::data_cache::PrefetchPattern;
