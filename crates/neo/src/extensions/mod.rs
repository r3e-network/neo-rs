//! Extension helpers mirroring the C# `Neo.Extensions` namespace.
//!
//! The modules in this folder provide trait implementations that add
//! convenience helpers to primitive types and core Neo structures so that
//! they behave exactly like the .NET extensions used by the C# reference
//! implementation.

pub mod byte;
pub mod collections;
pub mod log_level;
pub mod memory;
pub mod span;

pub mod io;
// Additional namespaces from the C# implementation (e.g. SmartContract,
// VM, etc.) will be brought online once the dependent subsystems are fully
// restored.

pub use byte::ByteExtensions;
pub use collections::CollectionExtensions;
pub use log_level::LogLevel;
pub use memory::ReadOnlyMemoryExtensions;
pub use span::SpanExtensions;

pub use io::binary_reader::BinaryReaderExtensions;
pub use io::binary_writer::BinaryWriterExtensions;
pub use io::memory_reader::MemoryReaderExtensions;
pub use io::serializable::SerializableExtensions;
