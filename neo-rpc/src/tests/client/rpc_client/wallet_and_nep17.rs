use super::*;

#[tokio::test]
async fn validate_address_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("validateaddressasync") else {
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
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"validateaddress".*"params"\s*:\s*\[\s*"{address}"\s*\]"#,
        address = escape(&address)
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
        .validate_address(&address)
        .await
        .expect("validate address");
    assert_eq!(actual.to_json(), expected.clone());
}

#[tokio::test]
async fn import_priv_key_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("importprivkeyasync") else {
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
    let wif = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("wif");

    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"importprivkey".*"params"\s*:\s*\[\s*"{wif}"\s*\]"#,
        wif = escape(&wif)
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
    let actual = client.import_priv_key(&wif).await.expect("import");
    assert_eq!(actual.to_json(), expected.clone());
}

#[tokio::test]
async fn get_unclaimed_gas_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getunclaimedgasasync") else {
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
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"getunclaimedgas".*"params"\s*:\s*\[\s*"{address}"\s*\]"#,
        address = escape(&address)
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
        .get_unclaimed_gas(&address)
        .await
        .expect("unclaimed gas");
    assert_eq!(actual.to_json(), expected.clone());
}

#[tokio::test]
async fn get_nep17_transfers_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getnep17transfersasync") else {
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
    let start_time = params
        .get(1)
        .and_then(|value| value.as_number())
        .map(|value| value as u64);
    let end_time = params
        .get(2)
        .and_then(|value| value.as_number())
        .map(|value| value as u64);

    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"getnep17transfers".*"params"\s*:\s*\[\s*"{address}""#,
        address = escape(&address)
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
        .get_nep17_transfers(&address, start_time, end_time)
        .await
        .expect("nep17 transfers");
    let settings = ProtocolSettings::default_settings();
    assert_eq!(actual.to_json(&settings), expected.clone());
}

#[tokio::test]
async fn get_nep17_transfers_accepts_null_transfer_address() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getnep17transfersasync_with_null_transferaddress") else {
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
    let start_time = params
        .get(1)
        .and_then(|value| value.as_number())
        .map(|value| value as u64);
    let end_time = params
        .get(2)
        .and_then(|value| value.as_number())
        .map(|value| value as u64);

    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"getnep17transfers".*"params"\s*:\s*\[\s*"{address}""#,
        address = escape(&address)
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
        .get_nep17_transfers(&address, start_time, end_time)
        .await
        .expect("nep17 transfers");
    let settings = ProtocolSettings::default_settings();
    assert_eq!(actual.to_json(&settings), expected.clone());
}

#[tokio::test]
async fn get_nep17_balances_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getnep17balancesasync") else {
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
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"getnep17balances".*"params"\s*:\s*\[\s*"{address}"\s*\]"#,
        address = escape(&address)
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
        .get_nep17_balances(&address)
        .await
        .expect("nep17 balances");
    let settings = ProtocolSettings::default_settings();
    assert_eq!(actual.to_json(&settings), expected.clone());
}

#[tokio::test]
async fn send_to_address_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("sendtoaddressasync") else {
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
    let address = params
        .get(1)
        .and_then(|value| value.as_string())
        .expect("address");
    let amount = params
        .get(2)
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
        r#""method"\s*:\s*"sendtoaddress".*"params"\s*:\s*\[\s*"{asset}"\s*,\s*"{address}"\s*,\s*"{amount}"\s*\]"#,
        asset = escape(&asset_id),
        address = escape(&address),
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
        .send_to_address(&asset_id, &address, &amount)
        .await
        .expect("sendtoaddress");
    let actual_hash = actual
        .get("hash")
        .and_then(|value| value.as_string())
        .expect("hash");
    assert_eq!(actual_hash, expected_hash);
}

#[tokio::test]
async fn send_many_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("sendmanyasync") else {
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
    let from_addr = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("from");
    let outputs = params
        .get(1)
        .and_then(|value| value.as_array())
        .expect("outputs");
    let settings = ProtocolSettings::default_settings();
    let parsed_outputs = outputs
        .iter()
        .filter_map(|item| item.as_ref())
        .filter_map(|token| token.as_object())
        .map(|obj| RpcTransferOut::from_json(obj, &settings).expect("output"))
        .collect::<Vec<_>>();
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
    let escaped_from = escape(&from_addr);
    let first_asset = outputs
        .get(0)
        .and_then(|token| token.as_object())
        .and_then(|obj| obj.get("asset"))
        .and_then(|value| value.as_string())
        .expect("asset");
    let second_asset = outputs
        .get(1)
        .and_then(|token| token.as_object())
        .and_then(|obj| obj.get("asset"))
        .and_then(|value| value.as_string())
        .expect("asset");
    let body_re = format!(
        r#"(?s)"method"\s*:\s*"sendmany".*"params"\s*:\s*\[\s*"{escaped_from}".*"{first_asset}".*"{second_asset}".*\]"#,
        escaped_from = escaped_from,
        first_asset = escape(&first_asset),
        second_asset = escape(&second_asset),
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
        .send_many(&from_addr, &parsed_outputs)
        .await
        .expect("sendmany");
    let actual_hash = actual
        .get("hash")
        .and_then(|value| value.as_string())
        .expect("hash");
    assert_eq!(actual_hash, expected_hash);
}
