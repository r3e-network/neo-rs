//! Response construction helpers for Oracle RPC methods.

use serde_json::{Map, Value};

pub(super) fn submit_oracle_response_to_json() -> Value {
    Value::Object(Map::new())
}
