//! Neo Extensions Framework
//!
//! This crate provides extension points and utilities for the Neo blockchain implementation.
//! Matches C# Neo.Extensions exactly.

// Core extensions matching C# Neo.Extensions
pub mod assembly_extensions;
pub mod big_integer_extensions;
pub mod byte_array_comparer;
pub mod byte_array_equality_comparer;
pub mod byte_extensions;
pub mod date_time_extensions;
pub mod encoding;
pub mod error;
pub mod integer_extensions;
pub mod log_level;
pub mod plugin;
pub mod secure_string_extensions;
pub mod string_extensions;
pub mod utilities;
pub mod utility;

// Collections extensions matching C# Neo.Extensions.Collections
pub mod collections {
    pub mod collection_extensions;
    pub mod collections_utility;
    pub mod hash_map_extensions;
    pub mod hash_set_extensions;
    pub mod vec_extensions;

    pub use collection_extensions::CollectionExtensions;
    pub use collections_utility::Collections;
    pub use hash_map_extensions::HashMapExtensions;
    pub use hash_set_extensions::HashSetExtensions;
    pub use vec_extensions::VecExtensions;
}

// Exceptions extensions matching C# Neo.Extensions.Exceptions
pub mod exceptions {
    pub mod try_catch_extensions;
}

// Factories extensions matching C# Neo.Extensions.Factories
pub mod factories {
    pub mod random_number_factory;
}

// Net extensions matching C# Neo.Extensions.Net
pub mod net {
    pub mod ip_address_extensions;
}

// Re-export commonly used types
pub use assembly_extensions::AssemblyExtensions;
pub use big_integer_extensions::BigIntegerExtensions;
pub use byte_array_comparer::ByteArrayComparer;
pub use byte_array_equality_comparer::ByteArrayEqualityComparer;
pub use byte_extensions::ByteExtensions;
pub use date_time_extensions::DateTimeExtensions;
pub use encoding::Encoding;
pub use integer_extensions::IntegerExtensions;
pub use log_level::LogLevel;
pub use secure_string_extensions::{SecureString, SecureStringExtensions};
pub use string_extensions::StringExtensions;
pub use utility::Utility;

// Re-export collections
pub use collections::collection_extensions::CollectionExtensions;
pub use collections::collections_utility::Collections;
pub use collections::hash_map_extensions::HashMapExtensions;
pub use collections::hash_set_extensions::HashSetExtensions;
pub use collections::vec_extensions::VecExtensions;

// Re-export exceptions
pub use exceptions::try_catch_extensions::TryCatchExtensions;

// Re-export factories
pub use factories::random_number_factory::RandomNumberFactory;

// Re-export net
pub use error::{ExtensionError, ExtensionResult};
pub use net::ip_address_extensions::{IpAddressExtensions, IpEndPointExtensions};
pub use plugin::{Plugin, PluginCategory, PluginContext, PluginEvent, PluginInfo, PluginManager};
