//! # neo-serialization
//!
//! Canonical home for Neo's pure serialization helpers: compression
//! (`compress_lz4` / `decompress_lz4`), C#-compatible binary stack-item
//! codec (`BinarySerializer`), JSON stack-item codec (`JsonSerializer`), and
//! the in-memory storage providers (`MemoryStore` etc.).
//!
//! ## Layering
//!
//! Sits at **Layer 1 (protocol)**. Depends on:
//! - `neo-primitives`, `neo-error` (Layer 0)
//! - `neo-storage` (Layer 0) — `ReadOnlyStore` / `WriteStore` / `StoreProvider` for `providers`
//! - `neo-vm`, `neo-vm-rs` (Layer 0) — `StackItem` round-trip and opcode metadata
//! - `neo-io` (Layer 0) — `BinaryWriter` / `MemoryReader` for wire encoding
//!
//! Must **not** depend on `neo-core` or anything that needs a stateful
//! consensus engine: this is the pure wire/storage serializer set, exactly
//! the role `parity-scale-codec` and `ssz` play in their respective stacks.

#![doc(html_root_url = "https://docs.rs/neo-serialization/0.7.2")]

pub mod binary_serializer;
pub mod compression;
/// C#-compatible JSON token model and JSONPath support.
pub mod json;
pub mod json_serializer;
/// In-memory storage provider implementations used by serialization tests and fixtures.
pub mod providers;
pub mod serialization;

pub use binary_serializer::BinarySerializer;
pub use compression::{Compression, CompressionAlgorithm, CompressionResult};
pub use json_serializer::JsonSerializer;
pub use providers::{MemorySnapshot, MemoryStore, MemoryStoreProvider};
pub use serialization::{
    compress_data, decompress_data, deserialize, deserialize_json, deserialize_neo_binary,
    estimate_serialized_size, serialize, serialize_json, serialize_neo_binary,
    validate_serialization,
};
