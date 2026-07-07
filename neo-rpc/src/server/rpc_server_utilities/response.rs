//! Response construction helpers for utility RPC methods.

use serde_json::{Value, json};

pub(super) fn validate_address_to_json(address: &str, is_valid: bool) -> Value {
    json!({
        "address": address,
        "isvalid": is_valid})
}
