use neo_core::neo_system::NeoSystem;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::UInt160;
use neo_rpc::{RpcServer, RpcServerConfig};
use serde_json::Value;

fn is_valid(result: &Value) -> bool {
    result
        .get("isvalid")
        .and_then(Value::as_bool)
        .expect("validateaddress isvalid flag")
}

#[tokio::test(flavor = "multi_thread")]
async fn validate_address_uses_wallet_base58_check() {
    let system = NeoSystem::new(ProtocolSettings::default(), None, None).expect("system");
    let server = RpcServer::new(system, RpcServerConfig::default());

    let valid_address = UInt160::zero().to_address();
    assert!(is_valid(&server.validate_address(&valid_address)));

    let mut invalid_checksum = valid_address.clone();
    let last = invalid_checksum.pop().expect("address last char");
    invalid_checksum.push(if last == 'A' { 'B' } else { 'A' });
    assert!(!is_valid(&server.validate_address(&invalid_checksum)));

    let spaced = format!(" {valid_address} ");
    assert!(!is_valid(&server.validate_address(&spaced)));
}
