use super::submission::map_oracle_error;
use super::*;
use crate::server::rpc_error::RpcError;
use crate::server::rpc_server::RpcServer;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use neo_config::ProtocolSettings;
use neo_crypto::Secp256r1Crypto;
use neo_oracle_service::OracleServiceError;
use serde_json::Value;

fn find_handler<'a>(handlers: &'a [RpcHandler], name: &str) -> &'a RpcHandler {
    handlers
        .iter()
        .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
        .expect("handler present")
}

fn submit_params(pubkey: Vec<u8>) -> [Value; 4] {
    [
        Value::String(BASE64_STANDARD.encode(pubkey)),
        Value::Number(1u64.into()),
        Value::String(BASE64_STANDARD.encode([0x11; 64])),
        Value::String(BASE64_STANDARD.encode([0x22; 64])),
    ]
}

#[test]
fn submit_oracle_response_success_shape_is_empty_object() {
    let result = super::response::submit_oracle_response_to_json();
    let object = result.as_object().expect("object");

    assert!(object.is_empty());
}

#[test]
fn submit_oracle_response_rejects_invalid_public_key() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(
        system,
        crate::server::rpc_server_settings::RpcServerConfig::default(),
    );
    let handlers = RpcServerOracle::register_handlers();
    let handler = find_handler(&handlers, "submitoracleresponse");

    let params = submit_params(vec![0x02, 0x03]);
    let err = (handler.callback())(&server, &params).expect_err("invalid oracle public key");
    let rpc_error: RpcError = err.into();

    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
    assert_eq!(rpc_error.data(), Some("Invalid oracle public key"));
}

#[test]
fn submit_oracle_response_requires_enabled_service_after_valid_request() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(
        system,
        crate::server::rpc_server_settings::RpcServerConfig::default(),
    );
    let handlers = RpcServerOracle::register_handlers();
    let handler = find_handler(&handlers, "submitoracleresponse");
    let public_key = Secp256r1Crypto::derive_public_key(&[0x42; 32]).expect("derive public key");

    let params = submit_params(public_key);
    let err = (handler.callback())(&server, &params).expect_err("oracle disabled");
    let rpc_error: RpcError = err.into();

    assert_eq!(rpc_error.code(), RpcError::oracle_disabled().code());
}

#[test]
fn map_oracle_error_includes_signature_message() {
    let err = OracleServiceError::InvalidSignature("bad signature".to_string());
    let rpc = map_oracle_error(err);
    assert_eq!(rpc.code(), RpcError::invalid_signature().code());
    assert_eq!(rpc.data(), Some("bad signature"));
}

#[test]
fn map_oracle_error_includes_not_designated_message() {
    let err = OracleServiceError::NotDesignated("not oracle".to_string());
    let rpc = map_oracle_error(err);
    assert_eq!(rpc.code(), RpcError::oracle_not_designated_node().code());
    assert_eq!(rpc.data(), Some("not oracle"));
}
