//! Remote RPC payload decoding shared by node-side RPC integrations.
//!
//! Remote-ledger mode and fast-sync reference validation both ask RPC nodes for
//! raw serialized protocol payloads. Keeping hex/base64 normalization here gives
//! both paths the same decoding rules without duplicating protocol mechanics.

use anyhow::Context;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};

/// Decodes a raw serialized RPC payload returned as either hex or base64 text.
pub(in crate::node) fn decode_remote_serialized_payload<T>(
    raw_text: &str,
    label: &'static str,
    deserialize: impl Fn(&[u8]) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let trimmed = raw_text.trim();
    let prefer_hex = looks_like_hex_payload(trimmed);
    let first = if prefer_hex {
        RemotePayloadEncoding::Hex
    } else {
        RemotePayloadEncoding::Base64
    };
    let second = if prefer_hex {
        RemotePayloadEncoding::Base64
    } else {
        RemotePayloadEncoding::Hex
    };

    let first_error = match decode_remote_serialized_with(trimmed, label, first, &deserialize) {
        Ok(payload) => return Ok(payload),
        Err(err) => err,
    };
    match decode_remote_serialized_with(trimmed, label, second, &deserialize) {
        Ok(payload) => Ok(payload),
        Err(second_error) => Err(anyhow::anyhow!(
            "remote RPC {label} was neither valid {first} nor {second}: {first_error}; {second_error}"
        )),
    }
}

fn decode_remote_serialized_with<T>(
    text: &str,
    label: &'static str,
    encoding: RemotePayloadEncoding,
    deserialize: impl Fn(&[u8]) -> anyhow::Result<T>,
) -> anyhow::Result<T> {
    let bytes = match encoding {
        RemotePayloadEncoding::Base64 => BASE64_STANDARD
            .decode(text)
            .with_context(|| format!("decoding remote RPC {label} base64"))?,
        RemotePayloadEncoding::Hex => {
            let hex_text = text.strip_prefix("0x").unwrap_or(text);
            neo_primitives::hex_util::decode_hex(hex_text)
                .with_context(|| format!("decoding remote RPC {label} hex"))?
        }
    };
    deserialize(&bytes)
}

#[derive(Clone, Copy)]
enum RemotePayloadEncoding {
    Base64,
    Hex,
}

impl std::fmt::Display for RemotePayloadEncoding {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Base64 => f.write_str("base64"),
            Self::Hex => f.write_str("hex"),
        }
    }
}

fn looks_like_hex_payload(text: &str) -> bool {
    let hex_text = text.strip_prefix("0x").unwrap_or(text);
    !hex_text.is_empty()
        && hex_text.len().is_multiple_of(2)
        && hex_text.bytes().all(|byte| byte.is_ascii_hexdigit())
}
