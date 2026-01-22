use super::super::{NeoFsAuth, OracleServiceSettings};
use super::signing::sign_neofs_bearer;
use crate::wallets::KeyPair;

pub(crate) fn build_neofs_auth(
    settings: &OracleServiceSettings,
    oracle_key: Option<&KeyPair>,
) -> NeoFsAuth {
    let token = settings
        .neofs_bearer_token
        .as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let mut signature = settings
        .neofs_bearer_signature
        .as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());
    let mut signature_key = settings
        .neofs_bearer_signature_key
        .as_ref()
        .map(|v| v.trim().to_string())
        .filter(|v| !v.is_empty());

    if settings.neofs_auto_sign_bearer
        && signature.is_none()
        && signature_key.is_none()
        && token.as_deref().map(strip_bearer_prefix).is_some()
    {
        if let (Some(token_value), Some(key)) = (token.as_deref(), oracle_key) {
            if let Some((sig, key_bytes)) = sign_neofs_bearer(
                strip_bearer_prefix(token_value),
                key,
                settings.neofs_wallet_connect,
            ) {
                signature = Some(hex::encode(sig));
                signature_key = Some(hex::encode(key_bytes));
            }
        }
    }

    NeoFsAuth {
        token,
        signature,
        signature_key,
        wallet_connect: settings.neofs_wallet_connect,
    }
}

pub(crate) fn strip_bearer_prefix(value: &str) -> &str {
    let trimmed = value.trim();
    if trimmed.len() >= 7 && trimmed[..7].eq_ignore_ascii_case("bearer ") {
        &trimmed[7..]
    } else {
        trimmed
    }
}
