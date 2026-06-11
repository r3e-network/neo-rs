//! # neo-extensions
//!
//! Trait extensions that mirror the C# `Neo.Extensions` namespace. These are
//! small, focused extension traits that augment the standard library /
//! `neo-io` / `neo-primitives` types with Neo-protocol-specific helpers
//! (LZ4 span compression, byte-slice deserialization, log-level routing,
//! error aliases).
//!
//! ## Layering
//!
//! Sits in **Layer 1 (utility)**. Depends only on:
//! - `neo-primitives` (Layer 0) — for `LogLevel`.
//! - `neo-error` (Layer 0) — for the `CoreError` re-export alias.
//! - `neo-io` (Layer 0) — for `Serializable` + LZ4 compression helpers.
//! - `neo-crypto` (Layer 0) — placeholder for future crypto extensions.
//!
//! Must **not** depend on `neo-core` or any higher-layer crate. This is the
//! same rule polkadot-sdk and reth follow for their small utility crates:
//! extension traits are leaf-level and must not import the runtime that
//! ultimately consumes them.
//!
//! ## Modules
//!
//! - [`byte`] — `ByteLz4Extensions` for LZ4 / deserialization helpers on
//!   `&[u8]` and `Vec<u8>`.
//! - [`span`] — `SpanExtensions` for LZ4 helpers on `&[u8]` / `Vec<u8>`
//!   (mirrors `Neo.Extensions.SpanExtensions`).
//! - [`memory`] — `ReadOnlyMemoryExtensions` for read-only byte-span
//!   deserialization (mirrors `Neo.Extensions.ReadOnlyMemoryExtensions`).
//! - [`error`] — Re-export of `CoreError` and `ExtensionResult` alias.
//! - [`utility`] — `ExtensionsUtility` global log-level + log-handler
//!   registration (mirrors `Neo.Extensions.Utility`).
//!
//! ## Re-export alias for `neo-core` callers
//!
//! `neo-core` historically re-exports the old `crate::extensions::*` paths.
//! After this crate was extracted, `neo-core` keeps the same module surface
//! (re-exports from `neo-extensions`) so the historical
//! `neo_core::extensions::ByteLz4Extensions` import paths continue to work.

#![doc(html_root_url = "https://docs.rs/neo-extensions/0.7.2")]
#![deny(unsafe_code)]
#![warn(missing_docs)]

pub mod byte;
pub mod compression;
pub mod error;
pub mod memory;
pub mod span;
pub mod utility;

// Re-export the `BinaryReaderExtensions` / `BinaryWriterExtensions` /
// `MemoryReaderExtensions` / `SerializableCollectionExtensions` /
// `SerializableExtensions` types from `neo-io` so historical
// `neo_core::extensions::io::*` paths keep resolving.
pub mod io {
    pub use neo_io::extensions::{
        binary_reader::BinaryReaderExtensions,
        binary_writer::BinaryWriterExtensions,
        memory_reader::MemoryReaderExtensions,
        serializable::{SerializableCollectionExtensions, SerializableExtensions},
    };
}

pub use byte::ByteLz4Extensions;
pub use compression::{
    compress_lz4, decompress_lz4, CompressionResult, COMPRESSION_MIN_SIZE, COMPRESSION_THRESHOLD,
};
pub use error::{CoreError, ExtensionResult};
pub use memory::ReadOnlyMemoryExtensions;
pub use span::SpanExtensions;
pub use utility::ExtensionsUtility;

// Re-export the `log_level` factory from `neo-primitives` for callers
// that historically used `crate::extensions::log_level`.
pub use neo_primitives::log_level;


/// Implements `Ord` and `PartialOrd` for a struct by comparing its fields in order.
/// This mirrors the C# `neo_core::impl_ord_by_fields!` macro that
/// plugin types used to derive ordering for the legacy RocksDB
/// key codecs. The Rust ports re-use it for the same purpose.
#[macro_export]
macro_rules! impl_ord_by_fields {
    ($name:ident, $($field:ident),+) => {
        impl PartialOrd for $name {
            fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
                Some(self.cmp(other))
            }
        }
        impl Ord for $name {
            fn cmp(&self, other: &Self) -> std::cmp::Ordering {
                use std::cmp::Ordering;
                $(match self.$field.cmp(&other.$field) {
                    Ordering::Equal => {},
                    other => return other,
                })+
                Ordering::Equal
            }
        }
        impl PartialEq for $name {
            fn eq(&self, other: &Self) -> bool {
                $(self.$field == other.$field)&&+
            }
        }
        impl Eq for $name {}
    };
}
