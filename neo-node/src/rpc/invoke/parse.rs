use neo_rpc::{RpcError, RpcParams};

pub fn parse_script_bytes(params: &RpcParams) -> Result<Vec<u8>, RpcError> {
    match params.as_value() {
        Some(serde_json::Value::Array(values)) => {
            let first = values
                .get(0)
                .and_then(|value| value.as_str())
                .ok_or_else(|| RpcError::invalid_params("script argument must be a hex string"))?;
            decode_script_hex(first)
        }
        Some(serde_json::Value::String(text)) => decode_script_hex(text.as_str()),
        Some(serde_json::Value::Object(map)) => {
            let script = map
                .get("script")
                .and_then(|value| value.as_str())
                .ok_or_else(|| {
                    RpcError::invalid_params("object params must include \"script\" string field")
                })?;
            decode_script_hex(script)
        }
        _ => Err(RpcError::invalid_params(
            "script argument is required and must be hex",
        )),
    }
}

fn decode_script_hex(input: &str) -> Result<Vec<u8>, RpcError> {
    let trimmed = input.trim();
    let without_prefix = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    hex::decode(without_prefix)
        .map_err(|_| RpcError::invalid_params("script must be a valid hex string"))
}
