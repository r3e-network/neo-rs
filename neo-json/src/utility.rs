//! Utility - matches C# Neo.Json.Utility exactly

/// Utility functions for JSON handling
pub struct JsonUtility;

impl JsonUtility {
    /// Strict UTF-8 encoding/decoding
    /// In Rust, String is always valid UTF-8, so this is handled automatically
    pub fn strict_utf8_decode(bytes: &[u8]) -> Result<String, std::string::FromUtf8Error> {
        String::from_utf8(bytes.to_vec())
    }

    /// Strict UTF-8 encoding
    #[must_use] 
    pub fn strict_utf8_encode(s: &str) -> Vec<u8> {
        s.as_bytes().to_vec()
    }
}
