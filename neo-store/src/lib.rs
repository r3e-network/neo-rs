//! Storage abstractions backing the Neo N3 Rust node.
//!
//! The crate currently exposes an in-memory implementation used for tests and
//! deterministic simulations, plus a Sled-backed persistent store powering the
//! node's consensus snapshots. Both implementations share the [`Store`] trait.
//! Design goals and API surface are documented in
//! `docs/specs/neo-modules.md#neo-store`.

mod columns;
mod error;
mod memory;
#[cfg(feature = "sled")] mod sled_store;
mod traits;

pub use columns::{BlockRecord, Blocks, HashKey, HeaderRecord, Headers, HeightKey};
pub use error::StoreError;
pub use memory::{MemorySnapshot, MemoryStore};
#[cfg(feature = "sled")] pub use sled_store::SledStore;
pub use traits::{BatchOp, Column, ColumnId, Store, StoreExt, WriteBatch};
