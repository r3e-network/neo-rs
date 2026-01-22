use base64::Engine as _;

pub(crate) fn push_json_field(output: &mut String, first: &mut bool, name: &str, value: &str) {
    if !*first {
        output.push_str(", ");
    } else {
        *first = false;
    }
    output.push('"');
    output.push_str(name);
    output.push_str("\": ");
    output.push_str(value);
}

pub(crate) fn json_string(value: &str) -> String {
    serde_json::to_string(value).unwrap_or_else(|_| "\"\"".to_string())
}

pub(crate) fn json_u64_string(value: u64) -> String {
    json_string(&value.to_string())
}

pub(crate) fn base64_from_base58(value: &str, expected_len: Option<usize>) -> Option<String> {
    let decoded = bs58::decode(value).into_vec().ok()?;
    if let Some(expected_len) = expected_len {
        if decoded.len() != expected_len {
            return None;
        }
    }
    if decoded.is_empty() {
        return None;
    }
    Some(base64::engine::general_purpose::STANDARD.encode(decoded))
}

pub(crate) fn normalize_neofs_hex_header(value: &str) -> String {
    let trimmed = value.trim();
    let normalized = trimmed.strip_prefix("0x").unwrap_or(trimmed);
    if is_hex(normalized) {
        return normalized.to_string();
    }
    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(normalized) {
        if !decoded.is_empty() {
            return hex::encode(decoded);
        }
    }
    trimmed.to_string()
}

fn is_hex(value: &str) -> bool {
    if value.is_empty() || value.len() % 2 != 0 {
        return false;
    }
    value
        .bytes()
        .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f' | b'A'..=b'F'))
}

pub(crate) fn header_str(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(|value| value.to_string())
}
