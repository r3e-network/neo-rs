//! Extension helpers mirroring the C# `Neo.Extensions` namespace.
//!
//! The modules in this folder provide trait implementations that add
//! convenience helpers to primitive types and core Neo structures so that
//! they behave exactly like the .NET extensions used by the C# reference
//! implementation.

pub mod byte;
pub mod byte_extensions;
pub mod error;
pub mod log_level;
pub mod memory;
pub mod span;
pub mod utility;

pub mod io;

pub use byte::ByteLz4Extensions;
pub use byte_extensions::ByteExtensions;
pub use error::{ExtensionError, ExtensionResult};
pub use log_level::LogLevel;
pub use memory::ReadOnlyMemoryExtensions;
pub use span::SpanExtensions;
pub use utility::ExtensionsUtility;

pub use io::binary_reader::BinaryReaderExtensions;
pub use io::binary_writer::BinaryWriterExtensions;
pub use io::memory_reader::MemoryReaderExtensions;
pub use io::serializable::{SerializableCollectionExtensions, SerializableExtensions};
