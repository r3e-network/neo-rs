use super::*;

#[tokio::test]
async fn get_connection_count_matches_fixture() {
    let Some(fixture) =
        mock_no_param_fixture("getconnectioncountasync", "getconnectioncount").await
    else {
        return;
    };
    let expected = fixture
        .response
        .get("result")
        .and_then(|value| value.as_number())
        .expect("result") as u32;

    let actual = fixture
        .client
        .get_connection_count()
        .await
        .expect("connection count");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_committee_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getcommitteeasync") else {
        return;
    };
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_array())
        .expect("result")
        .iter()
        .filter_map(|item| item.as_ref())
        .filter_map(|token| token.as_string())
        .collect::<Vec<_>>();

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getcommittee".*"params"\s*:\s*\[\s*\]"#;
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
    let actual = client.get_committee().await.expect("committee");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_next_block_validators_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getnextblockvalidatorsasync") else {
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
        .map(|obj| RpcValidator::from_json(obj).expect("validator"))
        .collect::<Vec<_>>();

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getnextblockvalidators".*"params"\s*:\s*\[\s*\]"#;
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
        .get_next_block_validators()
        .await
        .expect("validators");
    assert_eq!(actual.len(), expected.len());
    assert_eq!(actual[0].public_key, expected[0].public_key);
    assert_eq!(actual[0].votes, expected[0].votes);
}

#[tokio::test]
async fn get_transaction_height_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("gettransactionheightasync") else {
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
    let hash = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("hash");
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_number())
        .expect("result") as u32;

    let mut server = Server::new_async().await;
    let escaped = escape(&hash);
    let body_re =
        format!(r#""method"\s*:\s*"gettransactionheight".*"params"\s*:\s*\[\s*"{escaped}"\s*\]"#);
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
        .get_transaction_height(&hash)
        .await
        .expect("transaction height");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_native_contracts_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getnativecontractsasync") else {
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
        .map(|obj| RpcContractState::from_json(obj).expect("contract state"))
        .collect::<Vec<_>>();

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getnativecontracts".*"params"\s*:\s*\[\s*\]"#;
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
        .get_native_contracts()
        .await
        .expect("native contracts");
    assert_eq!(actual.len(), expected.len());
    for (left, right) in actual.iter().zip(expected.iter()) {
        assert_eq!(left.contract_state, right.contract_state);
    }
}

#[tokio::test]
async fn list_plugins_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("listpluginsasync") else {
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
        .map(|obj| RpcPlugin::from_json(obj).expect("plugin"))
        .collect::<Vec<_>>();

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"listplugins".*"params"\s*:\s*\[\s*\]"#;
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
    let actual = client.get_plugins().await.expect("plugins");
    assert_eq!(actual.len(), expected.len());
    for (left, right) in actual.iter().zip(expected.iter()) {
        assert_eq!(left.name, right.name);
        assert_eq!(left.version, right.version);
        assert_eq!(left.interfaces, right.interfaces);
        assert_eq!(left.category, right.category);
    }
}
