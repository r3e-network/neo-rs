//! Neo JSON library - matches C# Neo.Json exactly

pub mod error;

// Module declarations matching C# files
pub mod j_array;
pub mod j_boolean;
pub mod j_container;
pub mod j_number;
pub mod j_object;
pub mod j_path_token;
pub mod j_path_token_type;
pub mod j_string;
pub mod j_token;
pub mod ordered_dictionary;
pub mod ordered_dictionary_key_collection;
pub mod ordered_dictionary_value_collection;
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
