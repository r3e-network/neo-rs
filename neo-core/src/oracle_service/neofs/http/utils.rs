use crate::network::p2p::payloads::OracleResponseCode;
use crate::UInt256;
use futures::StreamExt;
use reqwest::StatusCode;
use sha2::{Digest, Sha256};

pub(crate) fn normalize_neofs_endpoint(endpoint: &str) -> Result<String, String> {
    let trimmed = endpoint.trim();
    if trimmed.is_empty() {
        return Err("NeoFS endpoint not configured".to_string());
    }
    let normalized = if trimmed.contains("://") {
        trimmed.to_string()
    } else {
        format!("http://{}", trimmed)
    };
    Ok(normalized)
}

pub(crate) fn map_neofs_status(status: StatusCode) -> Option<OracleResponseCode> {
    if status == StatusCode::OK || status == StatusCode::PARTIAL_CONTENT {
        return None;
    }
    if status == StatusCode::NOT_FOUND {
        return Some(OracleResponseCode::NotFound);
    }
    if status == StatusCode::FORBIDDEN {
        return Some(OracleResponseCode::Forbidden);
    }
    if status == StatusCode::REQUEST_TIMEOUT || status == StatusCode::GATEWAY_TIMEOUT {
        return Some(OracleResponseCode::Timeout);
    }
    Some(OracleResponseCode::Error)
}

pub(super) async fn read_limited_body(
    response: reqwest::Response,
    max_len: usize,
) -> Result<Vec<u8>, OracleResponseCode> {
    if let Some(len) = response.content_length() {
        if len as usize > max_len {
            return Err(OracleResponseCode::ResponseTooLarge);
        }
    }

    let mut body = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| OracleResponseCode::Error)?;
        if body.len() + chunk.len() > max_len {
            return Err(OracleResponseCode::ResponseTooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    Ok(body)
}

pub(super) async fn hash_response_body(
    response: reqwest::Response,
) -> Result<UInt256, OracleResponseCode> {
    let mut hasher = Sha256::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|_| OracleResponseCode::Error)?;
        hasher.update(&chunk);
    }

    let digest = hasher.finalize();
    UInt256::from_bytes(&digest).map_err(|_| OracleResponseCode::Error)
}
