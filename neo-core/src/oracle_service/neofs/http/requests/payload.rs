use super::super::super::{NeoFsAuth, OracleNeoFsProtocol};
use super::super::utils::{map_neofs_status, read_limited_body};
use crate::network::p2p::payloads::OracleResponseCode;
use crate::network::p2p::payloads::oracle_response::MAX_RESULT_SIZE;

impl OracleNeoFsProtocol {
    pub(super) async fn fetch_payload(
        &self,
        auth: &NeoFsAuth,
        object_url: &str,
    ) -> (OracleResponseCode, String) {
        let Ok(builder) = self.request_builder(reqwest::Method::GET, object_url, auth) else {
            return (OracleResponseCode::Error, String::new());
        };
        let Ok(response) = builder.send().await else {
            return (OracleResponseCode::Timeout, String::new());
        };

        if let Some(code) = map_neofs_status(response.status()) {
            return (code, String::new());
        }

        let body = match read_limited_body(response, MAX_RESULT_SIZE).await {
            Ok(body) => body,
            Err(code) => return (code, String::new()),
        };
        match String::from_utf8(body) {
            Ok(text) => (OracleResponseCode::Success, text),
            Err(_) => (OracleResponseCode::Error, String::new()),
        }
    }
}
