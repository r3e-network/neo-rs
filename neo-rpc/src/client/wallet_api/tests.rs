use super::*;
use crate::client::test_helpers::{localhost_binding_permitted, rpc_response};
use base64::{engine::general_purpose, Engine as _};
use mockito::{Matcher, Server};
use neo_config::ProtocolSettings;
use neo_script_builder::ScriptBuilder;
use neo_json::{JObject, JToken};
use neo_primitives::UInt256;
use neo_vm_rs::OpCode;
use regex::escape;
use reqwest::Url;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

fn load_rpc_case_result(name: &str) -> Option<JObject> {
    let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    path.push("..");
    path.push("neo_csharp");
    path.push("node");
    path.push("tests");
    path.push("Neo.Network.RPC.Tests");
    path.push("RpcTestCases.json");
    if !path.exists() {
        eprintln!(
            "SKIP: neo_csharp submodule not initialized ({})",
            path.display()
        );
        return None;
   }
    let payload = fs::read_to_string(&path).expect("read RpcTestCases.json");
    let token = JToken::parse(&payload, 128).expect("parse RpcTestCases.json");
    let cases = token
        .as_array()
        .expect("RpcTestCases.json should be an array");
    for entry in cases.children() {
        let token = entry.as_ref().expect("array entry");
        let obj = token.as_object().expect("case object");
        let case_name = obj
            .get("Name")
            .and_then(|value| value.as_string())
            .unwrap_or_default();
        if case_name.eq_ignore_ascii_case(name) {
            let response = obj
                .get("Response")
                .and_then(|value| value.as_object())
                .expect("case response");
            let result = response
                .get("result")
                .and_then(|value| value.as_object())
                .expect("case result");
            return Some(result.clone());
       }
   }
    eprintln!("SKIP: RpcTestCases.json missing case: {name}");
    None
}

fn invoke_response_integer(value: i64) -> String {
    let mut item = JObject::new();
    item.insert("type".to_string(), JToken::String("Integer".to_string()));
    item.insert("value".to_string(), JToken::String(value.to_string()));

    let mut result = JObject::new();
    result.insert("script".to_string(), JToken::String("00".to_string()));
    result.insert("state".to_string(), JToken::String("HALT".to_string()));
    result.insert("gasconsumed".to_string(), JToken::String("0".to_string()));
    result.insert(
        "stack".to_string(),
        JToken::Array(neo_json::JArray::from(vec![JToken::Object(item)])),
    );

    rpc_response(JToken::Object(result))
}

fn emit_argument(sb: &mut ScriptBuilder, arg: &serde_json::Value) -> Result<(), RpcError> {
    match arg {
        serde_json::Value::Null => {
            sb.emit_opcode(OpCode::PUSHNULL);
            Ok(())
       }
        serde_json::Value::Bool(b) => {
            sb.emit_push_bool(*b);
            Ok(())
       }
        serde_json::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                sb.emit_push_int(i);
                Ok(())
           } else if let Some(u) = n.as_u64() {
                sb.emit_push_int(u as i64);
                Ok(())
           } else {
                Err("Invalid number format".into())
           }
       }
        serde_json::Value::String(s) => {
            sb.emit_push(s.as_bytes());
            Ok(())
       }
        serde_json::Value::Array(arr) => {
            for item in arr.iter().rev() {
                emit_argument(sb, item)?;
           }
            sb.emit_push_int(arr.len() as i64);
            sb.emit_pack();
            Ok(())
       }
        _ => Err("Unsupported argument type".into())}
}

fn build_dynamic_call_script(
    script_hash: &UInt160,
    operation: &str,
    args: &[serde_json::Value],
) -> Vec<u8> {
    let mut sb = ScriptBuilder::new();

    if args.is_empty() {
        sb.emit_opcode(OpCode::NEWARRAY0);
   } else {
        for arg in args.iter().rev() {
            emit_argument(&mut sb, arg).expect("emit argument");
       }
        sb.emit_push_int(args.len() as i64);
        sb.emit_pack();
   }

    sb.emit_push_int(neo_execution::CallFlags::ALL.bits() as i64);
    sb.emit_push(operation.as_bytes());
    sb.emit_push(&script_hash.to_array());
    sb.emit_syscall("System.Contract.Call").expect("syscall");

    sb.to_array()
}

fn build_transfer_script(
    script_hash: &UInt160,
    from: &UInt160,
    to: &UInt160,
    amount: &BigInt,
    data: Option<serde_json::Value>,
    add_assert: bool,
) -> Vec<u8> {
    let mut sb = ScriptBuilder::new();

    if let Some(d) = data {
        emit_argument(&mut sb, &d).expect("emit argument");
   } else {
        sb.emit_opcode(OpCode::PUSHNULL);
   }
    sb.emit_push_int(amount.to_i64().expect("amount"));
    sb.emit_push(&to.to_array());
    sb.emit_push(&from.to_array());
    sb.emit_push_int(4);
    sb.emit_push_int(neo_execution::CallFlags::ALL.bits() as i64);
    sb.emit_push(b"transfer");
    sb.emit_push(&script_hash.to_array());
    sb.emit_syscall("System.Contract.Call").expect("syscall");
    if add_assert {
        sb.emit_opcode(OpCode::ASSERT);
   }

    sb.to_array()
}

fn mock_invokescript(server: &mut Server, script_b64: &str, response_body: &str) {
    let pattern = format!(
        r#""method"\s*:\s*"invokescript".*"params"\s*:\s*\[\s*"{script}""#,
        script = escape(script_b64),
    );
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(pattern))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .expect(1)
        .create();
}

fn mock_invokescript_any(server: &mut Server, response_body: &str) {
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(r#""method"\s*:\s*"invokescript""#.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(response_body)
        .expect_at_least(1)
        .create();
}

fn mock_block_count(server: &mut Server, count: u32) {
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(r#""method"\s*:\s*"getblockcount""#.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(rpc_response(JToken::Number(count as f64)))
        .expect(1)
        .create();
}

fn mock_calculate_network_fee(server: &mut Server, fee: i64) {
    let mut result = JObject::new();
    result.insert("networkfee".to_string(), JToken::Number(fee as f64));
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(
            r#""method"\s*:\s*"calculatenetworkfee""#.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(rpc_response(JToken::Object(result)))
        .expect_at_least(1)
        .create();
}

fn mock_sendrawtransaction(server: &mut Server, hash: UInt256) {
    let mut result = JObject::new();
    result.insert("hash".to_string(), JToken::String(hash.to_string()));
    let _m = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(
            r#""method"\s*:\s*"sendrawtransaction""#.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(rpc_response(JToken::Object(result)))
        .expect(1)
        .create();
}

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
