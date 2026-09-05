use super::super::super::json::build_neofs_object_payload;
use super::super::super::proto::neofs_v2;
use super::super::super::{NeoFsAuth, OracleNeoFsProtocol};
use super::super::auth::{build_neofs_meta_header, build_neofs_request_verification_header};
use super::super::verify::{validate_neofs_response, verify_neofs_signature_bytes};
use crate::network::p2p::payloads::OracleResponseCode;
use crate::wallets::KeyPair;
use prost::Message;
use tonic::transport::Channel;

impl OracleNeoFsProtocol {
    pub(in super::super::super) async fn fetch_header_grpc(
        &self,
        client: &mut neofs_v2::object::object_service_client::ObjectServiceClient<Channel>,
        address: &neofs_v2::refs::Address,
        auth: &NeoFsAuth,
        oracle_key: &KeyPair,
    ) -> (OracleResponseCode, String) {
        let object = match self
            .fetch_header_object_grpc(client, address, auth, oracle_key)
            .await
        {
            Ok(object) => object,
            Err(msg) => return (OracleResponseCode::Error, msg),
        };
        let header = match object.header.as_ref() {
            Some(h) => h,
            None => {
                return (
                    OracleResponseCode::Error,
                    "object has no header".to_string(),
                );
            }
        };
        let payload = build_neofs_object_payload(header, &object.payload);
        use base64::Engine as _;
        let payload_b64 = base64::engine::general_purpose::STANDARD.encode(payload.encode_to_vec());
        (OracleResponseCode::Success, payload_b64)
    }

    pub(in super::super::super) async fn fetch_header_object_grpc(
        &self,
        client: &mut neofs_v2::object::object_service_client::ObjectServiceClient<Channel>,
        address: &neofs_v2::refs::Address,
        auth: &NeoFsAuth,
        oracle_key: &KeyPair,
    ) -> Result<neofs_v2::object::Object, String> {
        let meta = build_neofs_meta_header(auth)?;
        let body = neofs_v2::object::head_request::Body {
            address: Some(address.clone()),
            main_only: false,
            raw: false,
        };
        let verify = build_neofs_request_verification_header(&body, &meta, oracle_key)?;
        let request = neofs_v2::object::HeadRequest {
            body: Some(body),
            meta_header: Some(meta),
            verify_header: Some(verify),
        };

        let response = client.head(request).await.map_err(|_| "request failed")?;
        let response = response.into_inner();
        let body = response
            .body
            .ok_or_else(|| "missing response body".to_string())?;
        validate_neofs_response(
            &body,
            response.meta_header.as_ref(),
            response.verify_header.as_ref(),
        )?;

        match body.head {
            Some(neofs_v2::object::head_response::body::Head::Header(header_with_sig)) => {
                let header = header_with_sig
                    .header
                    .ok_or_else(|| "missing object header".to_string())?;
                let signature = header_with_sig
                    .signature
                    .ok_or_else(|| "missing object signature".to_string())?;
                let object_id = address
                    .object_id
                    .as_ref()
                    .ok_or_else(|| "missing object id".to_string())?;
                if !verify_neofs_signature_bytes(&signature, &object_id.encode_to_vec()) {
                    return Err("invalid object signature".to_string());
                }
                Ok(neofs_v2::object::Object {
                    object_id: Some(object_id.clone()),
                    signature: Some(signature),
                    header: Some(header),
                    payload: Vec::new(),
                })
            }
            Some(neofs_v2::object::head_response::body::Head::ShortHeader(_)) => {
                Err("unexpected short header".to_string())
            }
            Some(neofs_v2::object::head_response::body::Head::SplitInfo(_)) => {
                Err("split header response".to_string())
            }
            None => Err("missing header response".to_string()),
        }
    }
}
