//! Extension helpers mirroring the C# `Neo.Extensions` namespace.
//!
//! The modules in this folder provide trait implementations that add
//! convenience helpers to primitive types and core Neo structures so that
//! they behave exactly like the .NET extensions used by the C# reference
//! implementation.

pub mod byte;
pub mod error;

pub mod memory;
pub mod span;
pub mod utility;

pub mod io;

pub use neo_primitives::log_level;

pub use byte::ByteLz4Extensions;
pub use error::ExtensionResult;

pub use memory::ReadOnlyMemoryExtensions;
pub use span::SpanExtensions;
pub use utility::ExtensionsUtility;

pub use io::BinaryReaderExtensions;
pub use io::BinaryWriterExtensions;
pub use io::MemoryReaderExtensions;
pub use io::{SerializableCollectionExtensions, SerializableExtensions};
