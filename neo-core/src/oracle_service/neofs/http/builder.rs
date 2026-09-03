use super::super::auth::strip_bearer_prefix;
use super::super::json::normalize_neofs_hex_header;
use super::super::{NeoFsAuth, OracleNeoFsProtocol};

impl OracleNeoFsProtocol {
    pub(super) fn request_builder(
        &self,
        method: reqwest::Method,
        object_url: &str,
        auth: &NeoFsAuth,
    ) -> Result<reqwest::RequestBuilder, url::ParseError> {
        let mut url = reqwest::Url::parse(object_url)?;
        if auth.wallet_connect {
            url.query_pairs_mut().append_pair("walletConnect", "true");
        }

        let mut builder = self.client.request(method, url);
        if let Some(token) = auth
            .token
            .as_ref()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
        {
            let value = if strip_bearer_prefix(token) != token {
                token.to_string()
            } else {
                format!("Bearer {}", token)
            };
            builder = builder.header(reqwest::header::AUTHORIZATION, value);
        }
        if let Some(signature) = auth
            .signature
            .as_ref()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
        {
            let normalized = normalize_neofs_hex_header(signature);
            builder = builder.header("X-Bearer-Signature", normalized);
        }
        if let Some(key) = auth
            .signature_key
            .as_ref()
            .map(|v| v.trim())
            .filter(|v| !v.is_empty())
        {
            let normalized = normalize_neofs_hex_header(key);
            builder = builder.header("X-Bearer-Signature-Key", normalized);
        }

        Ok(builder)
    }
}
