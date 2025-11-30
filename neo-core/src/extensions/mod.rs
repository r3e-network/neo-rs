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
pub mod plugin;
pub mod span;
pub mod utility;

pub mod io;

pub use byte::ByteLz4Extensions;
pub use byte_extensions::ByteExtensions;
pub use error::{ExtensionError, ExtensionResult};
pub use log_level::LogLevel;
pub use memory::ReadOnlyMemoryExtensions;
pub use plugin::{
    broadcast_global_event, global_plugin_infos, initialise_global_runtime, plugins_directory,
    shutdown_global_runtime, Plugin, PluginBase, PluginCategory, PluginContext, PluginEvent,
    PluginInfo, PluginManager, PluginRegistration, PluginRuntime, UnhandledExceptionPolicy,
};
pub use span::SpanExtensions;
pub use utility::ExtensionsUtility;

pub use io::binary_reader::BinaryReaderExtensions;
pub use io::binary_writer::BinaryWriterExtensions;
pub use io::memory_reader::MemoryReaderExtensions;
pub use io::serializable::{SerializableCollectionExtensions, SerializableExtensions};
