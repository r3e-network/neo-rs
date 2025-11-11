mod fields;

use serde_json::Value;

use super::super::Signer;

impl Signer {
    pub fn to_json(&self) -> Value {
        fields::build_signer_json(self)
    }
}
