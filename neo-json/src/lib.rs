#![warn(missing_docs)]
//! Neo JSON library - matches C# Neo.Json exactly

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
