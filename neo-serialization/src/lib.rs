//! # neo-serialization
//!
//! Binary, JSON, compression, and provider serialization helpers.
//!
//! ## Boundary
//!
//! This codec crate owns serialization adapters and must not run services,
//! import blocks, or mutate ledger state.
//!
//! ## Contents
//!
//! - `binary_serializer`: binary serializer implementation.
//! - `compression`: Compression codecs and deterministic envelope helpers.
//! - `json`: JSON models and codecs for external service integration.
//! - `json_serializer`: JSON serializer implementation.
//! - `providers`: Provider implementations behind the crate public traits.
//! - `serialization`: serialization codecs and compatibility checks.

#![doc(html_root_url = "https://docs.rs/neo-serialization/0.10.0")]

#[path = "codec/binary_serializer.rs"]
pub mod binary_serializer;
pub mod compression;
/// C#-compatible JSON token model and JSONPath support.
pub mod json;
#[path = "codec/json_serializer.rs"]
pub mod json_serializer;
/// In-memory storage provider implementations used by serialization tests and fixtures.
pub mod providers;
#[path = "codec/serialization.rs"]
pub mod serialization;

pub use binary_serializer::BinarySerializer;
pub use compression::{Compression, CompressionAlgorithm, CompressionResult};
pub use json_serializer::JsonSerializer;
pub use providers::{MemorySnapshot, MemoryStore, MemoryStoreProvider};
pub use serialization::{
    compress_data, decompress_data, deserialize_json, deserialize_neo_binary, serialize_json,
    serialize_neo_binary,
};
