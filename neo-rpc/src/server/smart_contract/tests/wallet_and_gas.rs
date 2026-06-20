use super::*;

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_with_wallet_returns_signed_tx() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let settings = Arc::new(ProtocolSettings::default());
    let key_pair = KeyPair::generate().expect("key pair");
    let contract = signature_contract_for_keypair(&key_pair);
    let account = StandardWalletAccount::new_with_key(key_pair, Some(contract), settings, None);
    let account_hash = account.script_hash();
    fund_gas(&server.system(), account_hash, 100_000_000);
    server.set_wallet(Some(Arc::new(TestWallet {
        name: "test".to_string(),
        account: Arc::new(account),
    })));

    let signers = json!([{
         "signer": {
             "account": account_hash.to_string(),
             "scopes": "CalledByEntry"
        }
    }]);
    let params = [
        Value::String(NeoToken::new().hash().to_string()),
        Value::String("totalSupply".to_string()),
        Value::Array(Vec::new()),
        signers,
    ];
    let result = (invokefunction.callback())(&server, &params).expect("invoke result");
    assert!(result.get("tx").is_some());
    assert!(result.get("pendingsignature").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_with_watch_only_wallet_returns_pending_signature() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let settings = Arc::new(ProtocolSettings::default());
    let key_pair = KeyPair::generate().expect("key pair");
    let contract = signature_contract_for_keypair(&key_pair);
    let account_hash = contract.script_hash();
    let account = StandardWalletAccount::new_watch_only(account_hash, Some(contract), settings);
    fund_gas(&server.system(), account_hash, 100_000_000);
    server.set_wallet(Some(Arc::new(TestWallet {
        name: "test".to_string(),
        account: Arc::new(account),
    })));

    let signers = json!([{
         "signer": {
             "account": account_hash.to_string(),
             "scopes": "CalledByEntry"
        }
    }]);
    let params = [
        Value::String(NeoToken::new().hash().to_string()),
        Value::String("totalSupply".to_string()),
        Value::Array(Vec::new()),
        signers,
    ];
    let result = (invokefunction.callback())(&server, &params).expect("invoke result");
    let pending = result
        .get("pendingsignature")
        .and_then(Value::as_object)
        .expect("pending signature");
    let items = pending
        .get("items")
        .and_then(Value::as_object)
        .expect("items");
    assert!(items.contains_key(&account_hash.to_string()));
    assert!(result.get("tx").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn invokefunction_with_missing_wallet_account_sets_exception() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let invokefunction = find_handler(&handlers, "invokefunction");

    let settings = Arc::new(ProtocolSettings::default());
    let key_pair = KeyPair::generate().expect("key pair");
    let contract = signature_contract_for_keypair(&key_pair);
    let account = StandardWalletAccount::new_with_key(key_pair, Some(contract), settings, None);
    server.set_wallet(Some(Arc::new(TestWallet {
        name: "test".to_string(),
        account: Arc::new(account),
    })));

    let missing_account = UInt160::from_bytes(&[0x42; 20]).expect("missing account hash");
    let missing_address =
        address_helper::to_address(&missing_account, server.system().settings().address_version);

    let signers = json!([{
         "signer": {
             "account": missing_account.to_string(),
             "scopes": "CalledByEntry"
        }
    }]);
    let params = [
        Value::String(NeoToken::new().hash().to_string()),
        Value::String("totalSupply".to_string()),
        Value::Array(Vec::new()),
        signers,
    ];
    let result = (invokefunction.callback())(&server, &params).expect("invoke result");

    let exception = result
        .get("exception")
        .and_then(Value::as_str)
        .expect("exception");
    let expected = format!(
        "The smart contract or address {} ({}) is not found. If this is your wallet address and you want to sign a transaction with it, make sure you have opened this wallet.",
        missing_account.to_hex_string(),
        missing_address
    );
    assert_eq!(exception, expected);
    assert!(result.get("tx").is_none());
    assert!(result.get("pendingsignature").is_none());
}

#[tokio::test(flavor = "multi_thread")]
async fn get_unclaimed_gas_rejects_invalid_address() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let get_unclaimed_gas = find_handler(&handlers, "getunclaimedgas");

    let params = [Value::String("not-an-address".to_string())];
    let err = (get_unclaimed_gas.callback())(&server, &params).expect_err("invalid address");
    assert_eq!(err.code(), -32602);
}

#[tokio::test(flavor = "multi_thread")]
async fn get_unclaimed_gas_returns_address_string() {
    let server = make_server(RpcServerConfig::default());
    let handlers = RpcServerSmartContract::register_handlers();
    let get_unclaimed_gas = find_handler(&handlers, "getunclaimedgas");

    let address =
        address_helper::to_address(&UInt160::zero(), server.system().settings().address_version);
    let params = [Value::String(address.clone())];
    let result = (get_unclaimed_gas.callback())(&server, &params).expect("unclaimed gas");

    let address_value = result
        .get("address")
        .and_then(Value::as_str)
        .expect("address");
    assert_eq!(address_value, address);

    let unclaimed = result
        .get("unclaimed")
        .and_then(Value::as_str)
        .expect("unclaimed");
    assert!(unclaimed.parse::<f64>().is_ok());
}
