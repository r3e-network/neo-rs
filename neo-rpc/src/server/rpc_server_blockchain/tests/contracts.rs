use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn get_contract_state_roundtrips_hash_and_id() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcontractstate");

    let hash = UInt160::from_bytes(&[0x01u8; 20]).expect("hash");
    let contract = make_contract_state(42, hash, "TestContract");
    let mut store = system.store_cache();
    store_contract_state(&mut store, &contract);

    let params = [Value::String(hash.to_string())];
    let result = (handler.callback())(&server, &params).expect("get contract");
    assert_eq!(result, contract_state_to_json(&contract));

    let params = [Value::Number(42i64.into())];
    let result = (handler.callback())(&server, &params).expect("get contract by id");
    assert_eq!(result, contract_state_to_json(&contract));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_contract_state_roundtrips_native_name_and_id() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcontractstate");

    let registry = crate::server::native_queries::NativeQueries::native_registry();
    let contract = registry
        .get_by_name("ContractManagement")
        .expect("contract management");
    let state = contract
        .contract_state(&settings, 0)
        .expect("contract state");

    let mut store = system.store_cache();
    store_contract_state(&mut store, &state);

    let params = [Value::Number(state.id.into())];
    let result_by_id = (handler.callback())(&server, &params).expect("get by id");

    let params = [Value::String("ContractManagement".to_string())];
    let result_by_name = (handler.callback())(&server, &params).expect("get by name");
    assert_eq!(result_by_id, result_by_name);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_contract_state_resolves_native_case_insensitive() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcontractstate");

    let registry = crate::server::native_queries::NativeQueries::native_registry();
    let gas_contract = registry.get_by_name("GasToken").expect("gas token");
    let gas_state = gas_contract
        .contract_state(&system.settings(), 0)
        .expect("gas state");
    let gas_hash = gas_state.hash;

    let mut store = system.store_cache();
    store_contract_state(&mut store, &gas_state);

    for name in ["gastoken", "GASTOKEN", "GasToken"] {
        let params = [Value::String(name.to_string())];
        let result = (handler.callback())(&server, &params).expect("get gas state");
        let obj = result.as_object().expect("object");
        assert_eq!(
            obj.get("hash").and_then(Value::as_str).unwrap_or_default(),
            gas_hash.to_string()
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn get_contract_state_rejects_unknown_contract() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcontractstate");

    let params = [Value::String(
        UInt160::from_bytes(&[0x22u8; 20])
            .expect("hash")
            .to_string(),
    )];
    let err = (handler.callback())(&server, &params).expect_err("unknown contract");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_contract().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_contract_state_rejects_invalid_identifier() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcontractstate");

    let params = [Value::String("0xInvalidHashString".to_string())];
    let err = (handler.callback())(&server, &params).expect_err("invalid hash");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());

    let params = [Value::String("InvalidContractName".to_string())];
    let err = (handler.callback())(&server, &params).expect_err("invalid name");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_contract_state_rejects_null_identifier() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcontractstate");

    let params = [Value::Null];
    let err = (handler.callback())(&server, &params).expect_err("null params");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}
