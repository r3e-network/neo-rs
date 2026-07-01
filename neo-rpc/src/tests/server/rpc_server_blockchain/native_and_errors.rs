use super::*;
use crate::server::rpc_server_blockchain::responses::contract_state_to_json;
use neo_native_contracts::contract_management::ContractManagement;

#[tokio::test(flavor = "multi_thread")]
async fn get_committee_returns_snapshot_members() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcommittee");

    let mut store = system.store_cache();
    store_committee(&mut store, &settings.standby_committee);

    let result = (handler.callback())(&server, &[]).expect("committee");
    let array = result.as_array().expect("array");
    // C# `NeoToken.GetCommittee` returns the cached members sorted
    // ascending (`OrderBy(p => p)`), not in standby order.
    let mut expected_points = settings.standby_committee.clone();
    expected_points.sort();
    assert_eq!(array.len(), expected_points.len());
    for (value, point) in array.iter().zip(&expected_points) {
        assert_eq!(
            value.as_str().unwrap_or_default(),
            hex::encode(point.as_bytes())
        );
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn get_native_contracts_includes_gas_token() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getnativecontracts");

    let registry = crate::server::native_queries::NativeQueries::native_registry();
    let gas_contract = registry.get_by_name("GasToken").expect("gas token");
    let gas_state = gas_contract
        .contract_state(&settings, 0)
        .expect("gas state");
    let gas_hash = gas_state.hash;

    let mut store = system.store_cache();
    store_contract_state(&mut store, &gas_state);

    let result = (handler.callback())(&server, &[]).expect("native contracts");
    let array = result.as_array().expect("array");
    let has_gas = array.iter().any(|entry| {
        entry
            .as_object()
            .and_then(|obj| obj.get("hash").and_then(Value::as_str))
            .map(|hash| hash == gas_hash.to_string())
            .unwrap_or(false)
    });
    assert!(has_gas);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_native_contracts_returns_all_registered_states() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getnativecontracts");

    let registry = crate::server::native_queries::NativeQueries::native_registry();
    let store = system.store_cache();
    let mut expected = Vec::new();
    for contract in registry.contracts() {
        if let Some(state) =
            ContractManagement::get_contract_from_snapshot(store.data_cache(), &contract.hash())
                .expect("contract read")
        {
            expected.push(contract_state_to_json(&state));
        }
    }

    let result = (handler.callback())(&server, &[]).expect("native contracts");
    let result_array = result.as_array().expect("array");
    assert_eq!(result_array.len(), expected.len());

    let expected_by_hash: HashMap<String, Value> = expected
        .into_iter()
        .map(|value| {
            let hash = value
                .as_object()
                .and_then(|obj| obj.get("hash").and_then(Value::as_str))
                .expect("hash present")
                .to_string();
            (hash, value)
        })
        .collect();

    for value in result_array {
        let hash = value
            .as_object()
            .and_then(|obj| obj.get("hash").and_then(Value::as_str))
            .expect("hash present");
        let expected_value = expected_by_hash
            .get(hash)
            .unwrap_or_else(|| panic!("missing expected contract {}", hash));
        assert_eq!(value, expected_value);
    }
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_reports_unknown_block() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblock");

    let block = make_ledger_block(&system.store_cache(), 1, vec![make_transaction(5)]);
    let mut store = system.store_cache();
    store_block(&mut store, &block);

    let params = [Value::Number(999u64.into())];
    let err = (handler.callback())(&server, &params).expect_err("unknown index");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_block().code());

    let params = [Value::String(UInt256::from([0x55u8; 32]).to_string())];
    let err = (handler.callback())(&server, &params).expect_err("unknown hash");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_block().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_block_hash_rejects_unknown_height() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getblockhash");

    let block = make_ledger_block(&system.store_cache(), 1, vec![make_transaction(6)]);
    let mut store = system.store_cache();
    store_block(&mut store, &block);

    let params = [Value::Number(2u64.into())];
    let err = (handler.callback())(&server, &params).expect_err("unknown height");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_height().code());
}
