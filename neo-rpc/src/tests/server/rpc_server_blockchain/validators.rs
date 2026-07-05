use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn get_transaction_height_reports_confirmed_height() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "gettransactionheight");

    let tx = make_transaction(9);
    let block = make_ledger_block(&system.store_cache(), 2, vec![tx.clone()]);
    let mut store = system.store_cache();
    store_block(&mut store, &block);

    let params = [Value::String(tx.hash().to_string())];
    let result = (handler.callback())(&server, &params).expect("transaction height");
    assert_eq!(result.as_u64().unwrap_or_default(), 2);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_transaction_height_rejects_mempool_transaction() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "gettransactionheight");

    let keypair = KeyPair::from_private_key(&[0x23u8; 32]).expect("keypair");
    let account = keypair.script_hash();
    let mut store = system.store_cache();
    mint_gas(
        &mut store,
        &settings,
        account,
        BigInt::from(50_0000_0000i64),
    );
    store.commit();

    let tx = build_signed_transaction(&settings, &keypair, 1);
    let pool = system.mempool();
    {
        let pool = &pool;
        assert_eq!(
            pool.try_add(tx.clone(), store.data_cache()),
            VerifyResult::Succeed
        );
    }

    let params = [Value::String(tx.hash().to_string())];
    let err = (handler.callback())(&server, &params).expect_err("mempool tx");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_transaction().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_transaction_height_rejects_unknown_transaction() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "gettransactionheight");

    let params = [Value::String(UInt256::from([0x44u8; 32]).to_string())];
    let err = (handler.callback())(&server, &params).expect_err("unknown tx");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_transaction().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_transaction_height_rejects_null_identifier() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "gettransactionheight");

    let params = [Value::Null];
    let err = (handler.callback())(&server, &params).expect_err("null params");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_next_block_validators_returns_standby() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getnextblockvalidators");

    let result = (handler.callback())(&server, &[]).expect("validators");
    let array = result.as_array().expect("array");
    assert_eq!(array.len(), settings.validators_count as usize);
    let expected: std::collections::HashSet<String> = settings
        .standby_validators()
        .into_iter()
        .map(|validator| hex::encode(validator.as_bytes()))
        .collect();
    let received: std::collections::HashSet<String> = array
        .iter()
        .filter_map(|item| {
            item.as_object()?
                .get("publickey")
                .and_then(Value::as_str)
                .map(|value| value.to_string())
        })
        .collect();
    assert_eq!(expected, received);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_next_block_validators_reports_candidate_votes() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getnextblockvalidators");

    let candidate = settings
        .standby_committee
        .first()
        .expect("committee")
        .clone();
    let mut store = system.store_cache();
    store_candidate_state(&mut store, &candidate, true, BigInt::from(42));

    let result = (handler.callback())(&server, &[]).expect("validators");
    let array = result.as_array().expect("array");
    let key = hex::encode(candidate.as_bytes());
    let entry = array
        .iter()
        .find_map(|item| {
            let obj = item.as_object()?;
            let public_key = obj.get("publickey")?.as_str()?;
            (public_key == key).then_some(obj)
        })
        .expect("candidate entry");
    assert_eq!(
        entry
            .get("votes")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        42
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_next_block_validators_reports_unregistered_as_negative_one() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getnextblockvalidators");

    let candidate = settings
        .standby_committee
        .first()
        .expect("committee")
        .clone();
    let mut store = system.store_cache();
    store_candidate_state(&mut store, &candidate, false, BigInt::from(11));

    let result = (handler.callback())(&server, &[]).expect("validators");
    let array = result.as_array().expect("array");
    let key = hex::encode(candidate.as_bytes());
    let entry = array
        .iter()
        .find_map(|item| {
            let obj = item.as_object()?;
            let public_key = obj.get("publickey")?.as_str()?;
            (public_key == key).then_some(obj)
        })
        .expect("candidate entry");
    assert_eq!(
        entry
            .get("votes")
            .and_then(Value::as_i64)
            .unwrap_or_default(),
        -1
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_candidates_reports_registered_candidate() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcandidates");

    let candidate = settings
        .standby_committee
        .first()
        .expect("committee")
        .clone();
    let mut store = system.store_cache();
    store_candidate_state(&mut store, &candidate, true, BigInt::from(10_000));

    let result = (handler.callback())(&server, &[]).expect("candidates");
    let array = result.as_array().expect("array");
    let key = hex::encode(candidate.as_bytes());
    let entry = array
        .iter()
        .find_map(|item| {
            let obj = item.as_object()?;
            let public_key = obj.get("publickey")?.as_str()?;
            (public_key == key).then_some(obj)
        })
        .expect("candidate entry");
    assert_eq!(
        entry
            .get("votes")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        "10000"
    );
    assert!(
        entry
            .get("active")
            .and_then(Value::as_bool)
            .unwrap_or(false)
    );
}

#[tokio::test(flavor = "multi_thread")]
async fn get_candidates_skips_blocked_and_unregistered() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcandidates");

    let candidate_active = settings
        .standby_committee
        .first()
        .expect("candidate")
        .clone();
    let candidate_blocked = settings
        .standby_committee
        .get(1)
        .expect("candidate")
        .clone();
    let candidate_unregistered = settings
        .standby_committee
        .get(2)
        .expect("candidate")
        .clone();

    let blocked_account =
        neo_execution::Contract::create_signature_contract(candidate_blocked.clone()).script_hash();
    let mut store = system.store_cache();
    store_candidate_state(&mut store, &candidate_active, true, BigInt::from(7));
    store_candidate_state(&mut store, &candidate_blocked, true, BigInt::from(9));
    store_candidate_state(&mut store, &candidate_unregistered, false, BigInt::from(11));
    store_blocked_account(&mut store, &blocked_account);

    let result = (handler.callback())(&server, &[]).expect("candidates");
    let array = result.as_array().expect("array");
    let keys: std::collections::HashSet<String> = array
        .iter()
        .filter_map(|item| {
            item.as_object()?
                .get("publickey")
                .and_then(Value::as_str)
                .map(|value| value.to_string())
        })
        .collect();

    assert!(keys.contains(&hex::encode(candidate_active.as_bytes())));
    assert!(!keys.contains(&hex::encode(candidate_blocked.as_bytes())));
    assert!(!keys.contains(&hex::encode(candidate_unregistered.as_bytes())));
}

#[tokio::test(flavor = "multi_thread")]
async fn get_candidates_reports_internal_error_on_invalid_state() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getcandidates");

    let candidate = settings
        .standby_committee
        .first()
        .expect("candidate")
        .clone();
    let bytes = serialize_test_stack_value(&StackValue::ByteString(vec![0x01]));
    let mut store = system.store_cache();
    store_candidate_state_raw(&mut store, &candidate, bytes);

    let err = (handler.callback())(&server, &[]).expect_err("invalid state");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::internal_server_error().code());
    assert_eq!(rpc_error.data(), Some("Can't get candidates."));
}
