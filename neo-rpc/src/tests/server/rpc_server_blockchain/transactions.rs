use super::*;
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_transaction_from_mempool() {
    let settings = ProtocolSettings::default();
    let system = crate::server::test_support::test_system(settings.clone());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawtransaction");

    let keypair = KeyPair::from_private_key(&[0x21u8; 32]).expect("keypair");
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

    let params = [Value::String(tx.hash().to_string()), Value::Bool(false)];
    let result = (handler.callback())(&server, &params).expect("get raw tx");
    let bytes = BASE64_STANDARD
        .decode(result.as_str().expect("base64"))
        .expect("decode");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Transaction as Serializable>::deserialize(&mut reader).expect("tx");
    assert_eq!(decoded.hash(), tx.hash());

    let params = [Value::String(tx.hash().to_string()), Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("get raw tx verbose");
    let obj = result.as_object().expect("object");
    assert_eq!(
        obj.get("hash").and_then(Value::as_str).unwrap_or_default(),
        tx.hash().to_string()
    );
    assert!(obj.get("blockhash").is_none());
    assert!(obj.get("sysfee").is_some());
    assert!(obj.get("netfee").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_transaction_confirmed_in_block() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system.clone(), RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawtransaction");

    let tx = make_transaction(7);
    let block = make_ledger_block(&system.store_cache(), 1, vec![tx.clone()]);
    let block_hash = block.hash();
    let mut store = system.store_cache();
    store_block(&mut store, &block);

    let params = [Value::String(tx.hash().to_string()), Value::Bool(false)];
    let result = (handler.callback())(&server, &params).expect("get raw tx");
    let bytes = BASE64_STANDARD
        .decode(result.as_str().expect("base64"))
        .expect("decode");
    let mut reader = MemoryReader::new(&bytes);
    let decoded = <Transaction as Serializable>::deserialize(&mut reader).expect("tx");
    assert_eq!(decoded.hash(), tx.hash());

    let params = [Value::String(tx.hash().to_string()), Value::Bool(true)];
    let result = (handler.callback())(&server, &params).expect("get raw tx verbose");
    let obj = result.as_object().expect("object");
    assert_eq!(
        obj.get("blockhash")
            .and_then(Value::as_str)
            .unwrap_or_default(),
        block_hash.to_string()
    );
    assert_eq!(
        obj.get("confirmations")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        1
    );
    assert_eq!(
        obj.get("blocktime")
            .and_then(Value::as_u64)
            .unwrap_or_default(),
        1
    );
    assert!(obj.get("sysfee").is_some());
    assert!(obj.get("netfee").is_some());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_transaction_rejects_unknown_hash() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawtransaction");

    let params = [Value::String(UInt256::from([0x99u8; 32]).to_string())];
    let err = (handler.callback())(&server, &params).expect_err("unknown tx");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::unknown_transaction().code());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_raw_transaction_rejects_null_identifier() {
    let system = crate::server::test_support::test_system(ProtocolSettings::default());
    let server = RpcServer::new(system, RpcServerConfig::default());
    let handlers = RpcServerBlockchain::register_handlers();
    let handler = find_handler(&handlers, "getrawtransaction");

    let params = [Value::Null];
    let err = (handler.callback())(&server, &params).expect_err("null params");
    let rpc_error: RpcError = err.into();
    assert_eq!(rpc_error.code(), RpcError::invalid_params().code());
}
