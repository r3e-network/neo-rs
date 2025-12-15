use super::*;
use crate::server::rcp_server_settings::RpcServerConfig;
use neo_core::{NeoSystem, ProtocolSettings, UInt160};

use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use neo_vm::op_code::OpCode;
use serde_json::Value;

fn find_handler<'a>(
    handlers: &'a [crate::server::rpc_server::RpcHandler],
    name: &str,
) -> &'a crate::server::rpc_server::RpcHandler {
    handlers
        .iter()
        .find(|handler| handler.descriptor().name.eq_ignore_ascii_case(name))
        .expect("handler present")
}

#[tokio::test(flavor = "multi_thread")]
async fn invokescript_returns_fault_state_in_result() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
    let server = crate::server::rpc_server::RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokescript = find_handler(&handlers, "invokescript");

    let script = vec![OpCode::THROW as u8];
    let params = [Value::String(BASE64_STANDARD.encode(script))];
    let result = (invokescript.callback())(&server, &params).expect("invoke result");

    let state = result
        .get("state")
        .and_then(|value| value.as_str())
        .expect("state field");
    assert_eq!(state, "FAULT");

    let exception = result.get("exception").expect("exception field");
    assert!(
        exception.is_string(),
        "expected exception string on FAULT state, got {exception:?}"
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn invokecontractverify_returns_unknown_contract_for_missing_contract() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
    let server = crate::server::rpc_server::RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokecontractverify = find_handler(&handlers, "invokecontractverify");

    let unknown = UInt160::zero().to_string();
    let params = [Value::String(unknown)];
    let err = (invokecontractverify.callback())(&server, &params).expect_err("should error");
    assert_eq!(err.code(), -102);
}
