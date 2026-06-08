//! Storage types for Neo blockchain.
//!
//! This module provides the core storage types that match the C# Neo implementation:
//! - `StorageKey`: Keys for contract storage with contract ID and key bytes
//! - `StorageItem`: Values stored in contract storage
//! - `SeekDirection`: Direction for storage iteration
//! - `TrackState`: Cache tracking states for storage entries

pub mod seek;
pub mod storage_item;
pub mod storage_key;
pub mod track;

pub use seek::SeekDirection;
pub use storage_item::StorageItem;
pub use storage_key::StorageKey;
pub use track::TrackState;
