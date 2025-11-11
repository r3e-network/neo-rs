use alloc::string::String;

use serde_json::Value;

use super::Signer;

mod from_json;
mod parse;
mod to_json;

impl Signer {
    pub fn from_json(value: &Value) -> Result<Self, String> {
        from_json::signer_from_json(value)
    }
}
