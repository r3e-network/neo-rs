use super::super::super::{NeoFsAuth, NeoFsCommand, NeoFsRequest, OracleNeoFsProtocol};
use crate::network::p2p::payloads::oracle_response::MAX_RESULT_SIZE;
use crate::network::p2p::payloads::OracleResponseCode;

impl OracleNeoFsProtocol {
    pub(in super::super::super) async fn execute_request(
        &self,
        endpoint: &str,
        request: NeoFsRequest,
        auth: &NeoFsAuth,
    ) -> (OracleResponseCode, String) {
        let base = endpoint.trim_end_matches('/');
        let object_url = format!(
            "{}/v1/objects/{}/by_id/{}",
            base, request.container, request.object
        );

        match request.command {
            NeoFsCommand::Payload => self.fetch_payload(auth, &object_url).await,
            NeoFsCommand::Header => self.fetch_header(auth, &object_url).await,
            NeoFsCommand::Range(range) => {
                if range.length > MAX_RESULT_SIZE as u64 {
                    return (OracleResponseCode::ResponseTooLarge, String::new());
                }
                self.fetch_range(auth, &object_url, range).await
            }
            NeoFsCommand::Hash(range) => self.fetch_hash(auth, &object_url, range).await,
        }
    }
}
