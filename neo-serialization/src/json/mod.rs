//! # neo-serialization::json
//!
//! JSON models and codecs for external service integration.
//!
//! ## Boundary
//!
//! This module belongs to `neo-serialization`. This codec crate owns
//! serialization adapters and must not run services, import blocks, or mutate
//! ledger state.
//!
//! ## Contents
//!
//! - `error`: Typed error definitions and conversions.
//! - `escape`: JSON string escape helpers.
//! - `j_array`: JSON array model.
//! - `j_object`: JSON object model.
//! - `j_path_token`: JSON path token model.
//! - `j_path_token_type`: JSON path token identifiers.
//! - `j_token`: JSON token model.
//! - `ordered_dictionary`: ordered JSON object map.

/// Error types for the Neo JSON library.
pub mod error;

/// C#-compatible JSON string escaping (`JavaScriptEncoder.Default`).
pub mod escape;

/// JSON array type (matches C# `JArray`).
pub mod j_array;
/// JSON object type (matches C# `JObject`).
pub mod j_object;
/// JSON path token for JSONPath evaluation.
pub mod j_path_token;
/// Token types used by the JSONPath parser.
pub mod j_path_token_type;
/// Core JSON token enum (matches C# `JToken`).
pub mod j_token;
/// Insertion-order-preserving dictionary (matches C# `OrderedDictionary`).
pub mod ordered_dictionary;

// Re-exports for convenience (matching C# namespace exports)
pub use error::JsonError;
pub use escape::CSharpEscapeFormatter;
pub use j_array::JArray;
pub use j_object::JObject;
pub use j_path_token::JPathToken;
pub use j_path_token_type::JPathTokenType;
pub use j_token::{JToken, MAX_JSON_DEPTH, MAX_SAFE_INTEGER, MIN_SAFE_INTEGER};
pub use ordered_dictionary::OrderedDictionary;
