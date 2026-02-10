#![warn(missing_docs)]
//! Neo JSON library - matches C# Neo.Json exactly

/// Error types for the Neo JSON library.
pub mod error;

// Module declarations matching C# files
/// JSON array type (matches C# `JArray`).
pub mod j_array;
/// JSON boolean type (matches C# `JBoolean`).
pub mod j_boolean;
/// JSON container base type (matches C# `JContainer`).
pub mod j_container;
/// JSON number type (matches C# `JNumber`).
pub mod j_number;
/// JSON object type (matches C# `JObject`).
pub mod j_object;
/// JSON path token for JSONPath evaluation.
pub mod j_path_token;
/// Token types used by the JSONPath parser.
pub mod j_path_token_type;
/// JSON string type (matches C# `JString`).
pub mod j_string;
/// Core JSON token enum (matches C# `JToken`).
pub mod j_token;
/// Insertion-order-preserving dictionary (matches C# `OrderedDictionary`).
pub mod ordered_dictionary;
/// Key collection view for `OrderedDictionary`.
pub mod ordered_dictionary_key_collection;
/// Value collection view for `OrderedDictionary`.
pub mod ordered_dictionary_value_collection;
/// JSON encoding/decoding utilities.
pub mod utility;

// Re-exports for convenience (matching C# namespace exports)
pub use error::JsonError;
pub use j_array::JArray;
pub use j_boolean::JBoolean;
pub use j_container::JContainer;
pub use j_number::{JNumber, MAX_SAFE_INTEGER, MIN_SAFE_INTEGER};
pub use j_object::JObject;
pub use j_path_token::JPathToken;
pub use j_path_token_type::JPathTokenType;
pub use j_string::JString;
pub use j_token::JToken;
pub use ordered_dictionary::OrderedDictionary;
pub use ordered_dictionary_key_collection::KeyCollection;
pub use ordered_dictionary_value_collection::ValueCollection;
pub use utility::JsonUtility;
