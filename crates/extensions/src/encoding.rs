use base64::{engine::general_purpose, Engine as _};

/// Encoding helpers mirroring the C# Neo.Extensions.Encoding utilities.
pub struct Encoding;

impl Encoding {
    pub fn to_hex(bytes: &[u8]) -> String {
        hex::encode(bytes)
    }

    pub fn to_hex_string(bytes: &[u8]) -> String {
        format!("0x{}", hex::encode(bytes))
    }

    pub fn from_hex(input: &str) -> Result<Vec<u8>, String> {
        let trimmed = input.trim_start_matches("0x").trim_start_matches("0X");
        hex::decode(trimmed).map_err(|e| e.to_string())
    }

    pub fn is_valid_hex(input: &str) -> bool {
        Self::from_hex(input).is_ok()
    }

    pub fn to_base64(bytes: &[u8]) -> String {
        general_purpose::STANDARD.encode(bytes)
    }

    pub fn from_base64(input: &str) -> Result<Vec<u8>, String> {
        general_purpose::STANDARD
            .decode(input.as_bytes())
            .map_err(|e| e.to_string())
    }

    pub fn to_base64_url(bytes: &[u8]) -> String {
        general_purpose::URL_SAFE_NO_PAD.encode(bytes)
    }

    pub fn from_base64_url(input: &str) -> Result<Vec<u8>, String> {
        general_purpose::URL_SAFE_NO_PAD
            .decode(input.as_bytes())
            .map_err(|e| e.to_string())
    }

    pub fn is_valid_base64(input: &str) -> bool {
        Self::from_base64(input).is_ok()
    }

    pub fn from_utf8_string(value: &str) -> Vec<u8> {
        value.as_bytes().to_vec()
    }

    pub fn to_utf8_string(bytes: &[u8]) -> Result<String, String> {
        String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())
    }
}
