use super::super::auth::{build_neofs_meta_header, build_neofs_request_verification_header};
use super::super::super::proto::neofs_v2;
use super::super::super::{NeoFsAuth, NeoFsRange, OracleNeoFsProtocol};
use super::super::verify::validate_neofs_response;
use crate::network::p2p::payloads::OracleResponseCode;
use crate::wallets::KeyPair;
use crate::UInt256;
use tonic::transport::Channel;

impl OracleNeoFsProtocol {
    pub(in super::super::super) async fn fetch_hash_grpc(
        &self,
        client: &mut neofs_v2::object::object_service_client::ObjectServiceClient<Channel>,
        address: &neofs_v2::refs::Address,
        range: Option<NeoFsRange>,
        auth: &NeoFsAuth,
        oracle_key: &KeyPair,
    ) -> (OracleResponseCode, String) {
        match range {
            None => {
                let object = match self
                    .fetch_header_object_grpc(client, address, auth, oracle_key)
                    .await
                {
                    Ok(object) => object,
                    Err(_) => return (OracleResponseCode::Error, String::new()),
                };
                let header = match object.header.as_ref() {
                    Some(header) => header,
                    None => return (OracleResponseCode::Error, String::new()),
                };
                let checksum = match header.payload_hash.as_ref() {
                    Some(checksum) => checksum,
                    None => return (OracleResponseCode::Error, String::new()),
                };
                let hash =
                    UInt256::from_bytes(&checksum.sum).map_err(|_| OracleResponseCode::Error);
                match hash {
                    Ok(hash) => (OracleResponseCode::Success, format!("\"{}\"", hash)),
                    Err(code) => (code, String::new()),
                }
            }
            Some(range) => {
                let meta = match build_neofs_meta_header(auth) {
                    Ok(meta) => meta,
                    Err(_) => return (OracleResponseCode::Error, String::new()),
                };
                let body = neofs_v2::object::get_range_hash_request::Body {
                    address: Some(address.clone()),
                    ranges: vec![neofs_v2::object::Range {
                        offset: range.offset,
                        length: range.length,
                    }],
                    salt: Vec::new(),
                    r#type: neofs_v2::refs::ChecksumType::Sha256 as i32,
                };
                let verify = match build_neofs_request_verification_header(&body, &meta, oracle_key)
                {
                    Ok(verify) => verify,
                    Err(_) => return (OracleResponseCode::Error, String::new()),
                };
                let request = neofs_v2::object::GetRangeHashRequest {
                    body: Some(body),
                    meta_header: Some(meta),
                    verify_header: Some(verify),
                };

                let response = match client.get_range_hash(request).await {
                    Ok(response) => response.into_inner(),
                    Err(_) => return (OracleResponseCode::Error, String::new()),
                };
                let body = match response.body.as_ref() {
                    Some(body) => body,
                    None => return (OracleResponseCode::Error, String::new()),
                };
                if validate_neofs_response(
                    body,
                    response.meta_header.as_ref(),
                    response.verify_header.as_ref(),
                )
                .is_err()
                {
                    return (OracleResponseCode::Error, String::new());
                }
                if body.hash_list.is_empty() {
                    return (OracleResponseCode::Error, String::new());
                }
                let hash =
                    UInt256::from_bytes(&body.hash_list[0]).map_err(|_| OracleResponseCode::Error);
                match hash {
                    Ok(hash) => (OracleResponseCode::Success, format!("\"{}\"", hash)),
                    Err(code) => (code, String::new()),
                }
            }
        }
    }
}
