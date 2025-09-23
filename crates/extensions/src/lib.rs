//! Neo Extensions Framework
//!
//! This crate provides extension points and utilities for the Neo blockchain implementation.

pub mod collections;
pub mod encoding;
pub mod error;
pub mod plugin;
pub mod utilities;

// Core extensions moved from neo-core
pub mod byte_extensions;
pub mod uint160_extensions;

// Re-export commonly used types
pub use error::{ExtensionError, ExtensionResult};
pub use plugin::{Plugin, PluginCategory, PluginContext, PluginEvent, PluginInfo};

// Re-export core extensions
pub use byte_extensions::ByteExtensions;
pub use uint160_extensions::UInt160Extensions;
