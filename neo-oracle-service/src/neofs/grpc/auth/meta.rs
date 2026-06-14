use super::super::super::NeoFsAuth;
use super::super::super::proto::neofs_v2;
use neo_error::{CoreError, CoreResult};

const NEOFS_SDK_VERSION_MAJOR: u32 = 2;
const NEOFS_SDK_VERSION_MINOR: u32 = 11;

pub(crate) fn build_neofs_meta_header(
    auth: &NeoFsAuth,
) -> CoreResult<neofs_v2::session::RequestMetaHeader> {
    let mut meta = neofs_v2::session::RequestMetaHeader {
        version: Some(neofs_v2::refs::Version {
            major: NEOFS_SDK_VERSION_MAJOR,
            minor: NEOFS_SDK_VERSION_MINOR,
        }),
        ttl: 2,
        ..Default::default()
    };
    if let Some(token) = build_neofs_bearer_token(auth)? {
        meta.bearer_token = Some(token);
    }
    Ok(meta)
}

fn build_neofs_bearer_token(
    auth: &NeoFsAuth,
) -> CoreResult<Option<neofs_v2::acl::BearerToken>> {
    let token = auth
        .token
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let Some(token) = token else {
        return Ok(None);
    };
    let data = base64::engine::general_purpose::STANDARD
        .decode(strip_bearer_prefix(token))
        .map_err(|_| CoreError::other("invalid bearer token"))?;
    if data.is_empty() {
        return Ok(None);
    }

    let signature = auth
        .signature
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());
    let signature_key = auth
        .signature_key
        .as_ref()
        .map(|value| value.trim())
        .filter(|value| !value.is_empty());

    match (signature, signature_key) {
        (Some(signature), Some(signature_key)) => {
            let body = neofs_v2::acl::bearer_token::Body::decode(data.as_slice())
                .map_err(|_| CoreError::other("invalid bearer token body"))?;
            let signature_bytes = decode_neofs_signature_bytes(signature)?;
            let key_bytes = decode_neofs_signature_bytes(signature_key)?;
            let scheme = if auth.wallet_connect {
                neofs_v2::refs::SignatureScheme::EcdsaRfc6979Sha256WalletConnect as i32
            } else {
                neofs_v2::refs::SignatureScheme::EcdsaSha512 as i32
            };
            Ok(Some(neofs_v2::acl::BearerToken {
                body: Some(body),
                signature: Some(neofs_v2::refs::Signature {
                    key: key_bytes,
                    sign: signature_bytes,
                    scheme,
                }),
            }))
        }
        (None, None) => {
            let token = neofs_v2::acl::BearerToken::decode(data.as_slice())
                .map_err(|_| CoreError::other("invalid bearer token"))?;
            Ok(Some(token))
        }
        _ => Err(CoreError::other("missing bearer signature or key")),
    }
}

fn decode_neofs_signature_bytes(value: &str) -> CoreResult<Vec<u8>> {
    let trimmed = value.trim();
    let normalized = normalize_neofs_hex_header(trimmed);
    if let Ok(decoded) = hex::decode(&normalized) {
        return Ok(decoded);
    }
    base64::engine::general_purpose::STANDARD
        .decode(trimmed)
        .map_err(|_| CoreError::other("invalid neofs signature"))
}

use super::super::super::auth::strip_bearer_prefix;
use super::super::super::json::normalize_neofs_hex_header;
use base64::Engine as _;
use prost::Message;
