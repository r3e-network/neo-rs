use super::super::super::{NeoFsAuth, NeoFsRange, OracleNeoFsProtocol};
use super::super::utils::{hash_response_body, map_neofs_status};
use crate::network::p2p::payloads::OracleResponseCode;

impl OracleNeoFsProtocol {
    pub(super) async fn fetch_hash(
        &self,
        auth: &NeoFsAuth,
        object_url: &str,
        range: Option<NeoFsRange>,
    ) -> (OracleResponseCode, String) {
        let builder = if let Some(range) = range {
            if range.length == 0 {
                return (
                    OracleResponseCode::Error,
                    "object range is invalid (expected 'Offset|Length')".to_string(),
                );
            }
            let Some(end) = range.offset.checked_add(range.length.saturating_sub(1)) else {
                return (
                    OracleResponseCode::Error,
                    "object range is invalid (expected 'Offset|Length')".to_string(),
                );
            };
            let builder = match self.request_builder(reqwest::Method::GET, object_url, auth) {
                Ok(builder) => builder,
                Err(_) => return (OracleResponseCode::Error, String::new()),
            };
            builder.header(
                reqwest::header::RANGE,
                format!("bytes={}-{}", range.offset, end),
            )
        } else {
            match self.request_builder(reqwest::Method::GET, object_url, auth) {
                Ok(builder) => builder,
                Err(_) => return (OracleResponseCode::Error, String::new()),
            }
        };

        let response = match builder.send().await {
            Ok(response) => response,
            Err(_) => return (OracleResponseCode::Timeout, String::new()),
        };

        if let Some(code) = map_neofs_status(response.status()) {
            return (code, String::new());
        }

        let hash = match hash_response_body(response).await {
            Ok(hash) => hash,
            Err(code) => return (code, String::new()),
        };
        (OracleResponseCode::Success, format!("\"{}\"", hash))
    }
}
