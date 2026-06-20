use super::*;

#[tokio::test]
async fn wallet_api_wait_transaction_returns_confirmed_tx() {
    if !localhost_binding_permitted() {
        return;
    }

    let mut settings = ProtocolSettings::default_settings();
    settings.milliseconds_per_block = 2;
    let tx = Transaction::new();

    let Some(result_json) = load_rpc_case_result("getrawtransactionasync") else {
        return;
    };
    let response_body = rpc_response(JToken::Object(result_json.clone()));

    let mut server = Server::new_async().await;
    let _m_tx = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(
            r#""method"\s*:\s*"getrawtransaction""#.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .expect_at_least(1)
        .create();

    let url = Url::parse(&server.url()).expect("server url");
    let client = RpcClient::builder(url)
        .protocol_settings(settings.clone())
        .build()
        .expect("client");
    let api = WalletApi::new(Arc::new(client));

    let rpc_tx = api.wait_transaction(&tx).await.expect("wait tx");
    assert_eq!(rpc_tx.confirmations, Some(643));
    assert_eq!(
        rpc_tx.block_hash,
        result_json
            .get("blockhash")
            .and_then(|v| v.as_string())
            .and_then(|s| UInt256::parse(&s).ok())
    );
}

#[tokio::test]
async fn wallet_api_wait_transaction_times_out() {
    if !localhost_binding_permitted() {
        return;
    }

    let mut settings = ProtocolSettings::default_settings();
    settings.milliseconds_per_block = 2;
    let tx = Transaction::new();

    let Some(mut unconfirmed) = load_rpc_case_result("getrawtransactionasync") else {
        return;
    };
    for key in ["confirmations", "blockhash", "blocktime", "vmstate"] {
        unconfirmed.properties_mut().remove(&key.to_string());
    }
    let response_body = rpc_response(JToken::Object(unconfirmed));

    let mut server = Server::new_async().await;
    let _m_tx = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(
            r#""method"\s*:\s*"getrawtransaction""#.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .expect_at_least(1)
        .create();

    let url = Url::parse(&server.url()).expect("server url");
    let client = RpcClient::builder(url)
        .protocol_settings(settings)
        .build()
        .expect("client");
    let api = WalletApi::new(Arc::new(client));

    let err = api
        .wait_transaction_with_timeout(&tx, 1)
        .await
        .expect_err("timeout");
    assert!(
        err.to_string().contains("Timeout"),
        "expected timeout error, got: {err}"
    );
}
