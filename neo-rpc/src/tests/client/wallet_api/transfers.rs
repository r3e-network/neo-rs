use super::*;

#[tokio::test]
async fn wallet_api_claim_gas_sends_transaction_and_skips_assert() {
    if !localhost_binding_permitted() {
        return;
    }

    let settings = ProtocolSettings::default_settings();
    let key = KeyPair::from_wif("KyXwTh1hB76RRMquSvnxZrJzQx7h9nQP2PCRL38v6VDb5ip3nf1p")
        .expect("key pair");

    let mut server = Server::new_async().await;
    mock_block_count(&mut server, 2);
    mock_calculate_network_fee(&mut server, 0);
    mock_invokescript_any(&mut server, &invoke_response_integer(1_00000000));
    mock_sendrawtransaction(&mut server, UInt256::zero());

    let url = Url::parse(&server.url()).expect("server url");
    let client = RpcClient::builder(url)
        .protocol_settings(settings)
        .build()
        .expect("client");
    let api = WalletApi::new(Arc::new(client));

    let tx = api
        .claim_gas_with_assert(&key, false)
        .await
        .expect("claim gas");
    assert_ne!(tx.script().last().copied(), Some(OpCode::ASSERT.byte()));
}

#[tokio::test]
async fn wallet_api_claim_gas_accepts_wif_string() {
    if !localhost_binding_permitted() {
        return;
    }

    let settings = ProtocolSettings::default_settings();
    let wif = "KyXwTh1hB76RRMquSvnxZrJzQx7h9nQP2PCRL38v6VDb5ip3nf1p";

    let mut server = Server::new_async().await;
    mock_block_count(&mut server, 2);
    mock_calculate_network_fee(&mut server, 0);
    mock_invokescript_any(&mut server, &invoke_response_integer(1_00000000));
    mock_sendrawtransaction(&mut server, UInt256::zero());

    let url = Url::parse(&server.url()).expect("server url");
    let client = RpcClient::builder(url)
        .protocol_settings(settings)
        .build()
        .expect("client");
    let api = WalletApi::new(Arc::new(client));

    let tx = api
        .claim_gas_from_key_with_assert(wif, false)
        .await
        .expect("claim gas");
    assert_ne!(tx.script().last().copied(), Some(OpCode::ASSERT.byte()));
}

#[tokio::test]
async fn wallet_api_transfer_sends_transaction_and_returns_hash() {
    if !localhost_binding_permitted() {
        return;
    }

    let settings = ProtocolSettings::default_settings();
    let key = KeyPair::from_wif("KyXwTh1hB76RRMquSvnxZrJzQx7h9nQP2PCRL38v6VDb5ip3nf1p")
        .expect("key pair");
    let to_hash = UInt160::from_bytes(&[0x55u8; 20]).expect("to hash");
    let to_address = WalletHelper::to_address(&to_hash, settings.address_version);
    let expected_hash =
        UInt256::parse("0x0000000000000000000000000000000000000000000000000000000000000011")
            .expect("hash");

    let mut server = Server::new_async().await;
    mock_block_count(&mut server, 2);
    mock_calculate_network_fee(&mut server, 0);
    mock_invokescript_any(&mut server, &invoke_response_integer(1_00000000));
    mock_sendrawtransaction(&mut server, expected_hash);

    let url = Url::parse(&server.url()).expect("server url");
    let client = RpcClient::builder(url)
        .protocol_settings(settings)
        .build()
        .expect("client");
    let api = WalletApi::new(Arc::new(client));

    let (tx, hash) = api
        .transfer_with_assert(
            &gas_hash().to_string(),
            &key,
            &to_address,
            BigInt::from(100u64),
            None,
            true,
        )
        .await
        .expect("transfer");
    assert_eq!(hash, expected_hash.to_string());
    assert_eq!(tx.script().last().copied(), Some(OpCode::ASSERT.byte()));
}

#[tokio::test]
async fn wallet_api_transfer_decimal_from_key_converts_amount() {
    if !localhost_binding_permitted() {
        return;
    }

    let settings = ProtocolSettings::default_settings();
    let wif = "KyXwTh1hB76RRMquSvnxZrJzQx7h9nQP2PCRL38v6VDb5ip3nf1p";
    let key = KeyPair::from_wif(wif).expect("key pair");
    let sender = key.get_script_hash();
    let to_hash = UInt160::from_bytes(&[0x88u8; 20]).expect("to hash");
    let to_address = WalletHelper::to_address(&to_hash, settings.address_version);
    let expected_hash =
        UInt256::parse("0x0000000000000000000000000000000000000000000000000000000000000044")
            .expect("hash");

    let token_hash = gas_hash();
    let amount = BigDecimal::new(BigInt::from(100u64), 0);
    let amount_integer = BigInt::from(10_000000000u64);

    let decimals_script = build_dynamic_call_script(&token_hash, "decimals", &[]);
    let decimals_script_b64 = general_purpose::STANDARD.encode(decimals_script);
    let transfer_script =
        build_transfer_script(&token_hash, &sender, &to_hash, &amount_integer, None, true);
    let transfer_script_b64 = general_purpose::STANDARD.encode(&transfer_script);

    let balance_args = vec![serde_json::json!(sender.to_string())];
    let balance_script = build_dynamic_call_script(&token_hash, "balanceOf", &balance_args);
    let balance_script_b64 = general_purpose::STANDARD.encode(balance_script);

    let mut server = Server::new_async().await;
    mock_invokescript(
        &mut server,
        &decimals_script_b64,
        &invoke_response_integer(8),
    );
    mock_invokescript(
        &mut server,
        &transfer_script_b64,
        &invoke_response_integer(1_00000000),
    );
    mock_invokescript(
        &mut server,
        &balance_script_b64,
        &invoke_response_integer(1_00000000),
    );
    mock_block_count(&mut server, 2);
    mock_calculate_network_fee(&mut server, 0);
    mock_sendrawtransaction(&mut server, expected_hash);

    let url = Url::parse(&server.url()).expect("server url");
    let client = RpcClient::builder(url)
        .protocol_settings(settings)
        .build()
        .expect("client");
    let api = WalletApi::new(Arc::new(client));

    let (tx, hash) = api
        .transfer_decimal_from_key_with_assert(
            &token_hash.to_string(),
            wif,
            &to_address,
            amount,
            None,
            true,
        )
        .await
        .expect("transfer");

    assert_eq!(hash, expected_hash.to_string());
    assert_eq!(tx.script(), transfer_script.as_slice());
    assert_eq!(tx.script().last().copied(), Some(OpCode::ASSERT.byte()));
}

#[tokio::test]
async fn wallet_api_transfer_multi_sig_requires_enough_keys() {
    let key = KeyPair::from_wif("KyXwTh1hB76RRMquSvnxZrJzQx7h9nQP2PCRL38v6VDb5ip3nf1p")
        .expect("key pair");
    let public_key = key.get_public_key_point().expect("public key");
    let to = UInt160::from_bytes(&[0x44u8; 20]).expect("to hash");

    let url = Url::parse("http://localhost").expect("url");
    let client = RpcClient::builder(url).build().expect("client");
    let api = WalletApi::new(Arc::new(client));

    let err = api
        .transfer_multi_sig(
            &gas_hash().to_string(),
            2,
            vec![public_key],
            vec![key],
            &to,
            BigInt::from(100u64),
            None,
            true,
        )
        .await
        .expect_err("insufficient keys");
    assert_eq!(err.to_string(), "Need at least 2 KeyPairs for signing!");
}

#[tokio::test]
async fn wallet_api_transfer_multi_sig_sends_transaction() {
    if !localhost_binding_permitted() {
        return;
    }

    let settings = ProtocolSettings::default_settings();
    let key = KeyPair::from_wif("KyXwTh1hB76RRMquSvnxZrJzQx7h9nQP2PCRL38v6VDb5ip3nf1p")
        .expect("key pair");
    let public_key = key.get_public_key_point().expect("public key");
    let to = UInt160::from_bytes(&[0x66u8; 20]).expect("to hash");
    let expected_hash =
        UInt256::parse("0x0000000000000000000000000000000000000000000000000000000000000022")
            .expect("hash");

    let mut server = Server::new_async().await;
    mock_block_count(&mut server, 2);
    mock_calculate_network_fee(&mut server, 0);
    mock_invokescript_any(&mut server, &invoke_response_integer(1_00000000));
    mock_sendrawtransaction(&mut server, expected_hash);

    let url = Url::parse(&server.url()).expect("server url");
    let client = RpcClient::builder(url)
        .protocol_settings(settings)
        .build()
        .expect("client");
    let api = WalletApi::new(Arc::new(client));

    let (tx, hash) = api
        .transfer_multi_sig(
            &gas_hash().to_string(),
            1,
            vec![public_key],
            vec![key],
            &to,
            BigInt::from(100u64),
            None,
            true,
        )
        .await
        .expect("multi-sig transfer");

    assert_eq!(hash, expected_hash.to_string());
    assert_eq!(tx.script().last().copied(), Some(OpCode::ASSERT.byte()));
}

#[tokio::test]
async fn wallet_api_transfer_multi_sig_with_empty_string_data() {
    if !localhost_binding_permitted() {
        return;
    }

    let settings = ProtocolSettings::default_settings();
    let key = KeyPair::from_wif("KyXwTh1hB76RRMquSvnxZrJzQx7h9nQP2PCRL38v6VDb5ip3nf1p")
        .expect("key pair");
    let public_key = key.get_public_key_point().expect("public key");
    let to = UInt160::from_bytes(&[0x77u8; 20]).expect("to hash");
    let expected_hash =
        UInt256::parse("0x0000000000000000000000000000000000000000000000000000000000000033")
            .expect("hash");

    let sender =
        Contract::create_multi_sig_contract(1, std::slice::from_ref(&public_key)).script_hash();
    let amount = BigInt::from(100u64);
    let script = build_transfer_script(
        &gas_hash(),
        &sender,
        &to,
        &amount,
        Some(serde_json::Value::String(String::new())),
        true,
    );
    let script_b64 = general_purpose::STANDARD.encode(&script);

    let balance_args = vec![serde_json::json!(sender.to_string())];
    let balance_script = build_dynamic_call_script(&gas_hash(), "balanceOf", &balance_args);
    let balance_script_b64 = general_purpose::STANDARD.encode(balance_script);

    let mut server = Server::new_async().await;
    mock_invokescript(
        &mut server,
        &script_b64,
        &invoke_response_integer(1_00000000),
    );
    mock_invokescript(
        &mut server,
        &balance_script_b64,
        &invoke_response_integer(1_00000000),
    );
    mock_block_count(&mut server, 2);
    mock_calculate_network_fee(&mut server, 0);
    mock_sendrawtransaction(&mut server, expected_hash);

    let url = Url::parse(&server.url()).expect("server url");
    let client = RpcClient::builder(url)
        .protocol_settings(settings)
        .build()
        .expect("client");
    let api = WalletApi::new(Arc::new(client));

    let (tx, hash) = api
        .transfer_multi_sig(
            &gas_hash().to_string(),
            1,
            vec![public_key],
            vec![key],
            &to,
            amount,
            Some(serde_json::Value::String(String::new())),
            true,
        )
        .await
        .expect("multi-sig transfer");

    assert_eq!(hash, expected_hash.to_string());
    assert_eq!(tx.script(), script.as_slice());
    assert_eq!(tx.script().last().copied(), Some(OpCode::ASSERT.byte()));
}
