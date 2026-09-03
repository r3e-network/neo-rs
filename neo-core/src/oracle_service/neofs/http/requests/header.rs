use super::super::super::json::build_neofs_header_payload;
use super::super::super::{NeoFsAuth, OracleNeoFsProtocol};
use super::super::utils::map_neofs_status;
use crate::network::p2p::payloads::OracleResponseCode;

impl OracleNeoFsProtocol {
    pub(super) async fn fetch_header(
        &self,
        auth: &NeoFsAuth,
        object_url: &str,
    ) -> (OracleResponseCode, String) {
        let builder = match self.request_builder(reqwest::Method::HEAD, object_url, auth) {
            Ok(builder) => builder,
            Err(_) => return (OracleResponseCode::Error, String::new()),
        };
        let response = match builder.send().await {
            Ok(response) => response,
            Err(_) => return (OracleResponseCode::Timeout, String::new()),
        };

        if let Some(code) = map_neofs_status(response.status()) {
            return (code, String::new());
        }

        let payload = build_neofs_header_payload(response.headers());
        (OracleResponseCode::Success, payload)
    }
}
