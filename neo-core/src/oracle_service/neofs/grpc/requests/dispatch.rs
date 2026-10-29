use super::super::super::{NeoFsAuth, NeoFsCommand, NeoFsRequest, OracleNeoFsProtocol};
use super::super::auth::build_neofs_grpc_address;
use super::super::client::neofs_grpc_client;
use crate::network::p2p::payloads::OracleResponseCode;
use crate::network::p2p::payloads::oracle_response::MAX_RESULT_SIZE;
use crate::wallets::KeyPair;

impl OracleNeoFsProtocol {
    pub(in super::super::super) async fn execute_grpc_request(
        &self,
        endpoint: &str,
        request: NeoFsRequest,
        auth: &NeoFsAuth,
        oracle_key: &KeyPair,
    ) -> (OracleResponseCode, String) {
        let Ok(address) = build_neofs_grpc_address(&request) else {
            return (OracleResponseCode::Error, String::new());
        };
        let Ok(mut client) = neofs_grpc_client(endpoint).await else {
            return (OracleResponseCode::Error, String::new());
        };

        match request.command {
            NeoFsCommand::Payload => {
                self.fetch_payload_grpc(&mut client, &address, auth, oracle_key)
                    .await
            }
            NeoFsCommand::Header => {
                self.fetch_header_grpc(&mut client, &address, auth, oracle_key)
                    .await
            }
            NeoFsCommand::Range(range) => {
                if range.length > MAX_RESULT_SIZE as u64 {
                    return (OracleResponseCode::ResponseTooLarge, String::new());
                }
                self.fetch_range_grpc(&mut client, &address, range, auth, oracle_key)
                    .await
            }
            NeoFsCommand::Hash(range) => {
                self.fetch_hash_grpc(&mut client, &address, range, auth, oracle_key)
                    .await
            }
        }
    }
}
