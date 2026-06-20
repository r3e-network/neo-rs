use super::*;

#[tokio::test]
async fn get_best_block_hash_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getbestblockhashasync") else {
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
    let body_re = r#""method"\s*:\s*"getbestblockhash".*"params"\s*:\s*\[\s*\]"#;
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
    let actual = client.get_best_block_hash().await.expect("best block hash");
    assert_eq!(actual, expected);
}

#[tokio::test]
async fn get_block_verbose_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(cases) = load_rpc_cases("getblockasync") else {
        return;
    };
    for case in cases {
        let request = case
            .get("Request")
            .and_then(|value| value.as_object())
            .expect("case request");
        let params = request
            .get("params")
            .and_then(|value| value.as_array())
            .expect("case params");
        let param = params.get(0).expect("hash or index");
        let (hash_or_index, body_re) = if let Some(index) = param.as_number() {
            let index = index as u32;
            (
                index.to_string(),
                format!(
                    r#""method"\s*:\s*"getblock".*"params"\s*:\s*\[\s*{index}\s*,\s*true\s*\]"#
                ),
            )
        } else if let Some(hash) = param.as_string() {
            let escaped = escape(&hash);
            (
                hash.to_string(),
                format!(
                    r#""method"\s*:\s*"getblock".*"params"\s*:\s*\[\s*"{escaped}"\s*,\s*true\s*\]"#,
                    escaped = escaped
                ),
            )
        } else {
            panic!("invalid getblock param");
        };

        let response = case
            .get("Response")
            .and_then(|value| value.as_object())
            .expect("case response");
        let expected = response
            .get("result")
            .and_then(|value| value.as_object())
            .expect("result");

        let mut server = Server::new_async().await;
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
            .get_block_verbose(&hash_or_index)
            .await
            .expect("block");
        let settings = ProtocolSettings::default_settings();
        assert_eq!(actual.to_json(&settings), expected.clone());
    }
}

#[tokio::test]
async fn get_block_header_verbose_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(cases) = load_rpc_cases("getblockheaderasync") else {
        return;
    };
    for case in cases {
        let request = case
            .get("Request")
            .and_then(|value| value.as_object())
            .expect("case request");
        let params = request
            .get("params")
            .and_then(|value| value.as_array())
            .expect("case params");
        let param = params.get(0).expect("hash or index");
        let (hash_or_index, body_re) = if let Some(index) = param.as_number() {
            let index = index as u32;
            (
                index.to_string(),
                format!(
                    r#""method"\s*:\s*"getblockheader".*"params"\s*:\s*\[\s*{index}\s*,\s*true\s*\]"#
                ),
            )
        } else if let Some(hash) = param.as_string() {
            let escaped = escape(&hash);
            (
                hash.to_string(),
                format!(
                    r#""method"\s*:\s*"getblockheader".*"params"\s*:\s*\[\s*"{escaped}"\s*,\s*true\s*\]"#,
                    escaped = escaped
                ),
            )
        } else {
            panic!("invalid getblockheader param");
        };

        let response = case
            .get("Response")
            .and_then(|value| value.as_object())
            .expect("case response");
        let expected = response
            .get("result")
            .and_then(|value| value.as_object())
            .expect("result");

        let mut server = Server::new_async().await;
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
            .get_block_header_verbose(&hash_or_index)
            .await
            .expect("block header");
        let settings = ProtocolSettings::default_settings();
        assert_eq!(actual.to_json(&settings), expected.clone());
    }
}

#[tokio::test]
async fn get_transaction_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getrawtransactionasync") else {
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
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"getrawtransaction".*"params"\s*:\s*\[\s*"{hash}"\s*,\s*true\s*\]"#,
        hash = escape(&hash)
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
    let actual = client.get_transaction(&hash).await.expect("transaction");
    let settings = ProtocolSettings::default_settings();
    assert_eq!(actual.to_json(&settings), expected.clone());
}

#[tokio::test]
async fn invoke_function_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("invokefunctionasync") else {
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
    let contract = params
        .get(0)
        .and_then(|value| value.as_string())
        .expect("contract");
    let operation = params
        .get(1)
        .and_then(|value| value.as_string())
        .expect("operation");
    let stack_params = params
        .get(2)
        .and_then(|value| value.as_array())
        .expect("stack params");
    let args = stack_params
        .children()
        .iter()
        .filter_map(|item| item.clone())
        .collect::<Vec<_>>();

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
        r#""method"\s*:\s*"invokefunction".*"params"\s*:\s*\[\s*"{contract}".*"{operation}".*\]"#,
        contract = escape(&contract),
        operation = escape(&operation)
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
        .invoke_function(&contract, &operation, &args)
        .await
        .expect("invoke function");
    assert_eq!(actual.to_json(), expected.clone());
}

#[tokio::test]
async fn get_contract_state_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(cases) = load_rpc_cases("getcontractstateasync") else {
        return;
    };
    for case in cases {
        let request = case
            .get("Request")
            .and_then(|value| value.as_object())
            .expect("case request");
        let params = request
            .get("params")
            .and_then(|value| value.as_array())
            .expect("case params");
        let contract = params.get(0).expect("contract");

        let response = case
            .get("Response")
            .and_then(|value| value.as_object())
            .expect("case response");
        let expected = response
            .get("result")
            .and_then(|value| value.as_object())
            .expect("result");

        let (body_re, request) = if let Some(name) = contract.as_string() {
            let body_re = format!(
                r#""method"\s*:\s*"getcontractstate".*"params"\s*:\s*\[\s*"{contract}"\s*\]"#,
                contract = escape(&name)
            );
            (body_re, ContractStateRequest::Hash(name.to_string()))
        } else if let Some(id) = contract.as_number() {
            let id = id as i32;
            let body_re =
                format!(r#""method"\s*:\s*"getcontractstate".*"params"\s*:\s*\[\s*{id}\s*\]"#);
            (body_re, ContractStateRequest::Id(id))
        } else {
            panic!("invalid getcontractstate param");
        };

        let mut server = Server::new_async().await;
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
        let actual = match request {
            ContractStateRequest::Hash(hash) => client.get_contract_state(&hash).await,
            ContractStateRequest::Id(id) => client.get_contract_state_by_id(id).await,
        }
        .expect("contract state");
        assert_eq!(actual.to_json().expect("to json"), expected.clone());
    }
}

#[tokio::test]
async fn get_peers_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getpeersasync") else {
        return;
    };
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getpeers".*"params"\s*:\s*\[\s*\]"#;
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
    let actual = client.get_peers().await.expect("peers");
    assert_eq!(actual.to_json(), expected.clone());
}

#[tokio::test]
async fn get_version_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getversionasync") else {
        return;
    };
    let response = case
        .get("Response")
        .and_then(|value| value.as_object())
        .expect("case response");
    let expected = response
        .get("result")
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = r#""method"\s*:\s*"getversion".*"params"\s*:\s*\[\s*\]"#;
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
    let actual = client.get_version().await.expect("version");
    assert_eq!(actual.to_json(), expected.clone());
}

#[tokio::test]
async fn get_application_log_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getapplicationlogasync") else {
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
        .and_then(|value| value.as_object())
        .expect("result");

    let mut server = Server::new_async().await;
    let body_re = format!(
        r#""method"\s*:\s*"getapplicationlog".*"params"\s*:\s*\[\s*"{hash}"\s*\]"#,
        hash = escape(&hash)
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
        .get_application_log(&hash)
        .await
        .expect("application log");
    assert_eq!(actual.to_json(), expected.clone());
}

#[tokio::test]
async fn get_application_log_with_trigger_matches_fixture() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some(case) = load_rpc_case("getapplicationlogasync_triggertype") else {
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
    let trigger = params
        .get(1)
        .and_then(|value| value.as_string())
        .expect("trigger");

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
        r#""method"\s*:\s*"getapplicationlog".*"params"\s*:\s*\[\s*"{hash}"\s*,\s*"{trigger}"\s*\]"#,
        hash = escape(&hash),
        trigger = escape(&trigger)
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
        .get_application_log_with_trigger(&hash, &trigger)
        .await
        .expect("application log");
    assert_eq!(actual.to_json(), expected.clone());
}
