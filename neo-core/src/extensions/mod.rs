//! Extension helpers mirroring the C# `Neo.Extensions` namespace.
//!
//! The modules in this folder provide trait implementations that add
//! convenience helpers to primitive types and core Neo structures so that
//! they behave exactly like the .NET extensions used by the C# reference
//! implementation.

/// Byte and byte-array extension helpers (e.g. LZ4 compression).
///
/// Kept local to neo-core: the compression methods return the historical
/// `CoreError`-wrapped `CompressionResult` contract asserted by core tests,
/// so this trait is not interchangeable with the `IoError`-flavored
/// `neo_io_crate::ByteLz4Extensions`.
pub mod byte;

/// General-purpose extension utilities.
pub mod utility;

/// Binary reader/writer extension helpers for Neo serialization.
pub mod io;

/// Compatibility module for the historical `extensions::log_level` path.
pub mod log_level {
    pub use neo_primitives::LogLevel;
}

pub use byte::ByteLz4Extensions;

pub use neo_io_crate::ReadOnlyMemoryExtensions;
pub use neo_io_crate::SpanExtensions;
pub use utility::ExtensionsUtility;

pub use io::BinaryReaderExtensions;
pub use io::BinaryWriterExtensions;
pub use io::MemoryReaderExtensions;
pub use io::{SerializableCollectionExtensions, SerializableExtensions};
