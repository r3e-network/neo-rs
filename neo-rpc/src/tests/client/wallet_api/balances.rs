use super::*;

#[tokio::test]
async fn wallet_api_get_unclaimed_gas_uses_block_count() {
    if !localhost_binding_permitted() {
        return;
    }

    let account = UInt160::zero();
    let settings = ProtocolSettings::default_settings();
    let address = WalletHelper::to_address(&account, settings.address_version);
    let block_count = 100u32;

    let args = vec![
        serde_json::json!(account.to_string()),
        serde_json::json!(block_count - 1),
    ];
    let script = build_dynamic_call_script(&neo_hash(), "unclaimedGas", &args);
    let script_b64 = general_purpose::STANDARD.encode(&script);

    let mut server = Server::new_async().await;
    let _m_block = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(r#""method"\s*:\s*"getblockcount""#.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(rpc_response(JToken::Number(block_count as f64)))
        .expect(1)
        .create();
    mock_invokescript(
        &mut server,
        &script_b64,
        &invoke_response_integer(110_000_000),
    );

    let url = Url::parse(&server.url()).expect("server url");
    let client = RpcClient::builder(url)
        .protocol_settings(settings)
        .build()
        .expect("client");
    let api = WalletApi::new(Arc::new(client));

    let balance = api
        .get_unclaimed_gas(&address)
        .await
        .expect("unclaimed gas");
    assert!((balance - 1.1).abs() < f64::EPSILON);
}

#[tokio::test]
async fn wallet_api_get_token_balance_reads_integer() {
    if !localhost_binding_permitted() {
        return;
    }

    let account = UInt160::zero();
    let settings = ProtocolSettings::default_settings();
    let address = WalletHelper::to_address(&account, settings.address_version);
    let token_hash = UInt160::from_bytes(&[0x11u8; 20]).expect("token hash");

    let args = vec![serde_json::json!(account.to_string())];
    let script = build_dynamic_call_script(&token_hash, "balanceOf", &args);
    let script_b64 = general_purpose::STANDARD.encode(&script);

    let mut server = Server::new_async().await;
    mock_invokescript(&mut server, &script_b64, &invoke_response_integer(42));

    let url = Url::parse(&server.url()).expect("server url");
    let client = RpcClient::builder(url)
        .protocol_settings(settings)
        .build()
        .expect("client");
    let api = WalletApi::new(Arc::new(client));

    let balance = api
        .get_token_balance(&token_hash.to_string(), &address)
        .await
        .expect("token balance");
    assert_eq!(balance, BigInt::from(42u64));
}

#[tokio::test]
async fn wallet_api_get_neo_and_gas_balances() {
    if !localhost_binding_permitted() {
        return;
    }

    let account = UInt160::from_bytes(&[0x22u8; 20]).expect("account hash");
    let settings = ProtocolSettings::default_settings();
    let address = WalletHelper::to_address(&account, settings.address_version);

    let args = vec![serde_json::json!(account.to_string())];
    let neo_script = build_dynamic_call_script(&neo_hash(), "balanceOf", &args);
    let gas_script = build_dynamic_call_script(&gas_hash(), "balanceOf", &args);
    let neo_script_b64 = general_purpose::STANDARD.encode(neo_script);
    let gas_script_b64 = general_purpose::STANDARD.encode(gas_script);

    let mut server = Server::new_async().await;
    mock_invokescript(
        &mut server,
        &neo_script_b64,
        &invoke_response_integer(1_00000000),
    );
    mock_invokescript(
        &mut server,
        &gas_script_b64,
        &invoke_response_integer(2_50000000),
    );

    let url = Url::parse(&server.url()).expect("server url");
    let client = RpcClient::builder(url)
        .protocol_settings(settings)
        .build()
        .expect("client");
    let api = WalletApi::new(Arc::new(client));

    let neo_balance = api.get_neo_balance(&address).await.expect("neo balance");
    assert_eq!(neo_balance, 1_00000000u32);

    let gas_balance = api.get_gas_balance(&address).await.expect("gas balance");
    assert!((gas_balance - 2.5).abs() < f64::EPSILON);
}

#[tokio::test]
async fn wallet_api_get_account_state_combines_balances() {
    if !localhost_binding_permitted() {
        return;
    }

    let account = UInt160::from_bytes(&[0x33u8; 20]).expect("account hash");
    let settings = ProtocolSettings::default_settings();
    let address = WalletHelper::to_address(&account, settings.address_version);
    let block_count = 77u32;

    let args = vec![serde_json::json!(account.to_string())];
    let neo_script = build_dynamic_call_script(&neo_hash(), "balanceOf", &args);
    let gas_script = build_dynamic_call_script(&gas_hash(), "balanceOf", &args);
    let unclaimed_args = vec![
        serde_json::json!(account.to_string()),
        serde_json::json!(block_count - 1),
    ];
    let unclaimed_script = build_dynamic_call_script(&neo_hash(), "unclaimedGas", &unclaimed_args);

    let neo_script_b64 = general_purpose::STANDARD.encode(neo_script);
    let gas_script_b64 = general_purpose::STANDARD.encode(gas_script);
    let unclaimed_script_b64 = general_purpose::STANDARD.encode(unclaimed_script);

    let mut server = Server::new_async().await;
    mock_invokescript(
        &mut server,
        &neo_script_b64,
        &invoke_response_integer(10_00000000),
    );
    mock_invokescript(
        &mut server,
        &gas_script_b64,
        &invoke_response_integer(2_10000000),
    );
    let _m_block = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(r#""method"\s*:\s*"getblockcount""#.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(rpc_response(JToken::Number(block_count as f64)))
        .expect(1)
        .create();
    mock_invokescript(
        &mut server,
        &unclaimed_script_b64,
        &invoke_response_integer(50_000000),
    );

    let url = Url::parse(&server.url()).expect("server url");
    let client = RpcClient::builder(url)
        .protocol_settings(settings)
        .build()
        .expect("client");
    let api = WalletApi::new(Arc::new(client));

    let state = api
        .get_account_state(&address)
        .await
        .expect("account state");
    assert_eq!(state.address, address);
    assert_eq!(state.neo_balance, 10_00000000u32);
    assert!((state.gas_balance - 2.1).abs() < f64::EPSILON);
    assert!((state.unclaimed_gas - 0.5).abs() < f64::EPSILON);
}
