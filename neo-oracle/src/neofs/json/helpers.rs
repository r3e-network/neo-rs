use base64::Engine as _;

use super::super::decode_raw_base58;

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

/// Renders one NeoFS token lifetime as a JSON object.
///
/// `exp`, `nbf` and `iat` all mean "no limit" when zero, so an unset bound is
/// omitted instead of being reported as an epoch-zero instant. The v1 and v2
/// protocol messages carry the same three fields as different generated types,
/// so callers pass the values rather than the message.
pub(crate) fn token_lifetime_json(exp: u64, nbf: u64, iat: u64) -> String {
    let mut out = String::from("{ ");
    let mut first = true;

    if exp != 0 {
        push_json_field(&mut out, &mut first, "exp", &json_u64_string(exp));
    }
    if nbf != 0 {
        push_json_field(&mut out, &mut first, "nbf", &json_u64_string(nbf));
    }
    if iat != 0 {
        push_json_field(&mut out, &mut first, "iat", &json_u64_string(iat));
    }

    if first {
        out.push('}');
    } else {
        out.push_str(" }");
    }
    out
}

pub(crate) fn base64_from_base58(value: &str, expected_len: Option<usize>) -> Option<String> {
    let decoded = decode_raw_base58(value, expected_len)?;
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
    if let Ok(decoded) = base64::engine::general_purpose::STANDARD.decode(normalized)
        && !decoded.is_empty()
    {
        return hex::encode(decoded);
    }
    trimmed.to_string()
}

fn is_hex(value: &str) -> bool {
    if value.is_empty() || !value.len().is_multiple_of(2) {
        return false;
    }
    value.bytes().all(|b: u8| b.is_ascii_hexdigit())
}

pub(crate) fn header_str(headers: &reqwest::header::HeaderMap, name: &str) -> Option<String> {
    headers
        .get(name)
        .and_then(|value| value.to_str().ok())
        .map(std::string::ToString::to_string)
}
