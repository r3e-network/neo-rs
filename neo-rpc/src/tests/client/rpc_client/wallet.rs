use super::*;

#[tokio::test]
async fn close_wallet_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("closewalletasync") else {
        return;
    };
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .map(|value| value.as_boolean())
        .unwrap_or(false);

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"closewallet".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client.close_wallet().await.expect("closewallet");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn open_wallet_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("openwalletasync") else {
        return;
    };
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let path = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("path");
    let password = params
        .get(1)
        .and_then(|value| value.as_string())
        .expect("password");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .map(|value| value.as_boolean())
        .unwrap_or(false);

    let mut server = Server::new_async().await;
    let escaped_path = escape(
        serde_json::to_string(&path)
            .expect("json path")
            .trim_matches('"'),
    );
    let escaped_password = escape(&password);
    let body_re = format!(
        r#""method"\s*:\s*"openwallet".*"params"\s*:\s*\[\s*"{escaped_path}"\s*,\s*"{escaped_password}"\s*\]"#
    );
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client
        .open_wallet(&path, &password)
        .await
        .expect("openwallet");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_new_address_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getnewaddressasync") else {
        return;
    };
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_string())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getnewaddress".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client.get_new_address().await.expect("new address");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn dump_priv_key_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("dumpprivkeyasync") else {
        return;
    };
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let address = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("address");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_string())
        .expect("result");

    let mut server = Server::new_async().await;
    let escaped = escape(&address);
    let body_re = format!(r#""method"\s*:\s*"dumpprivkey".*"params"\s*:\s*\[\s*"{escaped}"\s*\]"#);
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client.dump_priv_key(&address).await.expect("dumpprivkey");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn list_address_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("listaddressasync") else {
        return;
    };
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected_array = response
        .get("result")
        .and_then(|value| value.as_array())
        .expect("result");
    let expected = expected_array
        .iter()
        .filter_map(|item| item.as_ref())
        .filter_map(|token| token.as_object())
        .map(|obj| RpcAccount::from_json(obj).expect("account"))
        .collect::<Vec<_>>();

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"listaddress".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client.list_address().await.expect("listaddress");
    assert_eq!(actual.len(), expected.len());
    for (left, right) in actual.iter().zip(expected.iter()) {
        assert_eq!(left.address, right.address);
        assert_eq!(left.has_key, right.has_key);
        assert_eq!(left.label, right.label);
        assert_eq!(left.watch_only, right.watch_only);
    }
}

#[tokio::test]
async fn get_wallet_balance_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getwalletbalanceasync") else {
        return;
    };
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let asset_id = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("asset");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let balance = response
        .get("result")
        .and_then(|value| value.as_object())
        .and_then(|obj| obj.get("balance"))
        .and_then(|value| value.as_string())
        .expect("balance");
    let expected_value = BigInt::from_str(&balance).expect("parse balance");
    let expected = BigDecimal::new(expected_value, 8);

    let mut server = Server::new_async().await;
    let escaped_asset = escape(&asset_id);
    let wallet_body_re =
        format!(r#""method"\s*:\s*"getwalletbalance".*"params"\s*:\s*\[\s*"{escaped_asset}"\s*\]"#);
    let response_body = JToken::Object(response.clone()).to_string();
    let _m_wallet = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(wallet_body_re))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let invoke_body = r#"{"jsonrpc":"2.0","id":1,"result":{"script":"","state":"HALT","gasconsumed":"0","stack":[{"type":"Integer","value":"8"}]}}"#;
    let _m_invoke = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(r#""method"\s*:\s*"invokescript""#.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(invoke_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client
        .get_wallet_balance(&asset_id)
        .await
        .expect("wallet balance");
    assert_eq!(actual.value(), expected.value());
    assert_eq!(actual.decimals(), expected.decimals());
}

#[tokio::test]
async fn get_wallet_unclaimed_gas_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getwalletunclaimedgasasync") else {
        return;
    };
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let amount = response
        .get("result")
        .and_then(|value| value.as_string())
        .expect("result");
    let expected_value = BigInt::from_str(&amount).expect("parse amount");
    let expected = BigDecimal::new(expected_value, 8);

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getwalletunclaimedgas".*"params"\s*:\s*\[\s*\]"#;
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client
        .get_wallet_unclaimed_gas()
        .await
        .expect("wallet unclaimed gas");
    assert_eq!(actual.value(), expected.value());
    assert_eq!(actual.decimals(), expected.decimals());
}

#[tokio::test]
async fn send_from_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("sendfromasync") else {
        return;
    };
    let request = case
        .get("Request")
        .and_then(|value| value.as_object())
        .expect("case request");
    let params = request
        .get("params")
        .and_then(|value| value.as_array())
        .expect("case params");
    let asset_id = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("asset");
    let from_addr = params
        .get(1)
        .and_then(|value| value.as_string())
        .expect("from");
    let to_addr = params
        .get(2)
        .and_then(|value| value.as_string())
        .expect("to");
    let amount = params
        .get(3)
        .and_then(|value| value.as_string())
        .expect("amount");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected_hash = response
        .get("result")
        .and_then(|value| value.as_object())
        .and_then(|obj| obj.get("hash"))
        .and_then(|value| value.as_string())
        .expect("hash");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"sendfrom".*"params"\s*:\s*\[\s*"{asset}"\s*,\s*"{from}"\s*,\s*"{to}"\s*,\s*"{amount}"\s*\]"#,
        asset = escape(&asset_id),
        from = escape(&from_addr),
        to = escape(&to_addr),
        amount = escape(&amount),
    );
    let response_body = JToken::Object(response.clone()).to_string();
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(body_re))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .create();

    let url = Url::parse(&server.url()).unwrap();
    let client = RpcClient::builder(url).build().unwrap();
    let actual = client
        .send_from(&asset_id, &from_addr, &to_addr, &amount)
        .await
        .expect("sendfrom");
    let actual_hash = actual
        .get("hash")
        .and_then(|value| value.as_string())
        .expect("hash");
    assert_eq!(actual_hash, expected_hash);
}
