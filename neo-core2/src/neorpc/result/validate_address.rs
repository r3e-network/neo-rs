// ValidateAddress represents a result of the `validateaddress` call. Notice that
// Address is a serde_json::Value here because the server echoes back whatever address
// value a user has sent to it, even if it's not a string.
use serde::{Deserialize, Serialize};
use serde_json::Value;

#[derive(Serialize, Deserialize)]
pub struct ValidateAddress {
    pub address: Value,
    pub is_valid: bool,
}
