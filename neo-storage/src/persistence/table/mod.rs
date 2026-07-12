//! # neo-storage::persistence::table
//!
//! Statically dispatched logical tables and storage byte codecs.
//!
//! ## Boundary
//!
//! This module assigns Rust key/value types to the existing Neo data and
//! node-maintenance namespaces. It does not define protocol records or change
//! their persisted bytes; domain crates own those codecs.
//!
//! ## Contents
//!
//! - `codec`: Allocation-aware codec traits and built-in byte/integer codecs.
//! - `definition`: Logical table identity and namespace contracts.
//! - `provider`: Typed point reads over every concrete `Store` backend.

mod codec;
mod definition;
mod provider;

pub use codec::{
    BytesCodec, FixedBytesCodec, IntoTableBytes, StorageItemCodec, StorageKeyCodec, TableCodec,
    TableDecode, TableEncode, U32BeCodec, U64BeCodec,
};
pub use definition::{Table, TableNamespace};
pub use provider::TableProvider;

#[cfg(test)]
#[path = "../../tests/persistence/table.rs"]
mod tests;
