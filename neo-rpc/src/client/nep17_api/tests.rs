use super::*;
use crate::client::test_helpers::{localhost_binding_permitted, rpc_response};
use base64::{Engine as _, engine::general_purpose};
use mockito::{Matcher, Server};
use neo_native_contracts::GasToken;
use neo_native_contracts::NativeContract;
use neo_serialization::json::{JArray, JObject, JToken};
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::OpCode;
use regex::escape;
use reqwest::Url;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

fn invoke_response(stack: Vec<JToken>, gas_consumed: i64) -> String {
    let mut result = JObject::new();
    result.insert("script".to_string(), JToken::String("00".to_string()));
    result.insert("state".to_string(), JToken::String("HALT".to_string()));
    result.insert(
        "gasconsumed".to_string(),
        JToken::String(gas_consumed.to_string()),
    );
    result.insert("stack".to_string(), JToken::Array(JArray::from(stack)));

    rpc_response(JToken::Object(result))
}

fn invoke_response_integer(value: i64) -> String {
    let mut item = JObject::new();
    item.insert("type".to_string(), JToken::String("Integer".to_string()));
    item.insert("value".to_string(), JToken::String(value.to_string()));
    invoke_response(vec![JToken::Object(item)], 0)
}

fn invoke_response_empty(gas_consumed: i64) -> String {
    invoke_response(Vec::new(), gas_consumed)
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
        _ => Err("Unsupported argument type".into()),
    }
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

    sb.emit_push_int(CallFlags::ALL.bits() as i64);
    sb.emit_push(operation.as_bytes());
    sb.emit_push(&script_hash.to_array());
    sb.emit_syscall("System.Contract.Call").expect("syscall");

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

fn load_contract_state_case(manifest_name: &str) -> Option<(String, String)> {
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
        if !case_name.eq_ignore_ascii_case("getcontractstateasync") {
            continue;
        }

        let response = obj
            .get("Response")
            .and_then(|value| value.as_object())
            .expect("case response");
        let result = response
            .get("result")
            .and_then(|value| value.as_object())
            .expect("case result");
        let manifest = result
            .get("manifest")
            .and_then(|value| value.as_object())
            .expect("manifest object");
        let name = manifest
            .get("name")
            .and_then(|value| value.as_string())
            .unwrap_or_default();
        if !name.eq_ignore_ascii_case(manifest_name) {
            continue;
        }

        let request = obj
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
            .unwrap_or_default()
            .to_string();
        let contract = if UInt160::parse(&contract).is_ok() {
            contract
        } else {
            // `NativeRegistry::new()` is empty by design; resolve native
            // contract names through the canonical provider instead.
            use neo_execution::native_contract_provider::NativeContractProvider;
            neo_native_contracts::StandardNativeProvider::new()
                .all_native_contracts()
                .into_iter()
                .find(|native| native.name().eq_ignore_ascii_case(&contract))
                .map(|native| native.hash().to_string())
                .unwrap_or(contract)
        };

        return Some((contract, response.to_string()));
    }

    eprintln!("SKIP: RpcTestCases.json missing contract state for {manifest_name}");
    None
}

#[tokio::test]
async fn token_info_uses_contract_state_name() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some((contract_hash, contract_state_response)) = load_contract_state_case("GasToken")
    else {
        return;
    };
    let symbol = "GAS";
    let symbol_b64 = general_purpose::STANDARD.encode(symbol.as_bytes());

    let invoke_response = format!(
        r#"{{"jsonrpc":"2.0","id":1,"result":{{"script":"00","state":"HALT","gasconsumed":"0","stack":[{{"type":"ByteString","value":"{symbol_b64}"}},{{"type":"Integer","value":"8"}},{{"type":"Integer","value":"100000000"}}]}}}}"#
    );

    let mut server = Server::new_async().await;
    let _m_contract = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(
            r#""method"\s*:\s*"getcontractstate""#.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(contract_state_response)
        .create();
    let _m_invoke = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(r#""method"\s*:\s*"invokescript""#.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(invoke_response)
        .create();

    let url = Url::parse(&server.url()).expect("parse server url");
    let client = RpcClient::builder(url).build().expect("build client");
    let api = Nep17Api::new(Arc::new(client));

    let script_hash = UInt160::parse(&contract_hash).expect("script hash");
    let info = api.get_token_info(&script_hash).await.expect("token info");

    assert_eq!(info.name, "GasToken");
    assert_eq!(info.symbol, "GAS");
    assert_eq!(info.decimals, 8);
    assert_eq!(info.total_supply, BigInt::from(100000000u64));
}

#[tokio::test]
async fn token_info_by_contract_accepts_name() {
    if !localhost_binding_permitted() {
        return;
    }

    let Some((_contract_hash, contract_state_response)) = load_contract_state_case("GasToken")
    else {
        return;
    };
    let symbol_b64 = general_purpose::STANDARD.encode("GAS".as_bytes());

    let invoke_response = format!(
        r#"{{"jsonrpc":"2.0","id":1,"result":{{"script":"00","state":"HALT","gasconsumed":"0","stack":[{{"type":"ByteString","value":"{symbol_b64}"}},{{"type":"Integer","value":"8"}},{{"type":"Integer","value":"100000000"}}]}}}}"#
    );

    let mut server = Server::new_async().await;
    let _m_contract = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(
            r#""method"\s*:\s*"getcontractstate".*"GasToken""#.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(contract_state_response)
        .create();
    let _m_invoke = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(r#""method"\s*:\s*"invokescript""#.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(invoke_response)
        .create();

    let url = Url::parse(&server.url()).expect("parse server url");
    let client = RpcClient::builder(url).build().expect("build client");
    let api = Nep17Api::new(Arc::new(client));

    let info = api
        .get_token_info_by_contract("GasToken")
        .await
        .expect("token info");

    assert_eq!(info.name, "GasToken");
    assert_eq!(info.symbol, "GAS");
    assert_eq!(info.decimals, 8);
    assert_eq!(info.total_supply, BigInt::from(100000000u64));
}

#[tokio::test]
async fn token_info_with_balance_fetches_balance() {
    if !localhost_binding_permitted() {
        return;
    }

    let settings = neo_config::ProtocolSettings::default_settings();
    let address = WalletHelper::to_address(&UInt160::zero(), settings.address_version);
    let Some((contract_hash, contract_state_response)) = load_contract_state_case("GasToken")
    else {
        return;
    };
    let symbol_b64 = general_purpose::STANDARD.encode("GAS".as_bytes());

    let invoke_info_response = format!(
        r#"{{"jsonrpc":"2.0","id":1,"result":{{"script":"00","state":"HALT","gasconsumed":"0","stack":[{{"type":"ByteString","value":"{symbol_b64}"}},{{"type":"Integer","value":"8"}},{{"type":"Integer","value":"100000000"}}]}}}}"#
    );
    let invoke_balance_response = r#"{"jsonrpc":"2.0","id":1,"result":{"script":"00","state":"HALT","gasconsumed":"0","stack":[{"type":"Integer","value":"42"}]}}"#;

    let mut server = Server::new_async().await;
    let _m_contract = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(
            r#""method"\s*:\s*"getcontractstate""#.into(),
        ))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(contract_state_response)
        .create();

    let url = Url::parse(&server.url()).expect("parse server url");
    let client = RpcClient::builder(url).build().expect("build client");
    let api = Nep17Api::new(Arc::new(client));

    let script_hash = UInt160::parse(&contract_hash).expect("script hash");
    let mut info_script = Vec::new();
    info_script.extend(
        api.make_script(&script_hash, "symbol", vec![])
            .expect("symbol script"),
    );
    info_script.extend(
        api.make_script(&script_hash, "decimals", vec![])
            .expect("decimals script"),
    );
    info_script.extend(
        api.make_script(&script_hash, "totalSupply", vec![])
            .expect("total supply script"),
    );
    let info_script_b64 = general_purpose::STANDARD.encode(info_script);
    mock_invokescript(&mut server, &info_script_b64, &invoke_info_response);

    let account =
        WalletHelper::to_script_hash(&address, settings.address_version).expect("account");
    let balance_args = vec![serde_json::json!(account.to_string())];
    let balance_script = build_dynamic_call_script(&script_hash, "balanceOf", &balance_args);
    let balance_script_b64 = general_purpose::STANDARD.encode(balance_script);
    mock_invokescript(&mut server, &balance_script_b64, invoke_balance_response);
    let info = api
        .get_token_info_with_balance(&address, &script_hash)
        .await
        .expect("token info");

    assert_eq!(info.name, "GasToken");
    assert_eq!(info.balance, Some(BigInt::from(42u64)));
}

#[tokio::test]
async fn balance_of_reads_integer_stack_value() {
    if !localhost_binding_permitted() {
        return;
    }

    let invoke_response = r#"{"jsonrpc":"2.0","id":1,"result":{"script":"00","state":"HALT","gasconsumed":"0","stack":[{"type":"Integer","value":"42"}]}}"#;

    let mut server = Server::new_async().await;
    let _m_invoke = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(r#""method"\s*:\s*"invokescript""#.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(invoke_response)
        .create();

    let url = Url::parse(&server.url()).expect("parse server url");
    let client = RpcClient::builder(url).build().expect("build client");
    let api = Nep17Api::new(Arc::new(client));

    let script_hash = UInt160::zero();
    let account = UInt160::zero();
    let balance = api
        .balance_of(&script_hash, &account)
        .await
        .expect("balance");
    assert_eq!(balance, BigInt::from(42u64));
}

#[tokio::test]
async fn symbol_reads_string_stack_value() {
    if !localhost_binding_permitted() {
        return;
    }

    let symbol_b64 = general_purpose::STANDARD.encode("GAS".as_bytes());
    let invoke_response = format!(
        r#"{{"jsonrpc":"2.0","id":1,"result":{{"script":"00","state":"HALT","gasconsumed":"0","stack":[{{"type":"ByteString","value":"{symbol_b64}"}}]}}}}"#
    );

    let mut server = Server::new_async().await;
    let _m_invoke = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(r#""method"\s*:\s*"invokescript""#.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(invoke_response)
        .create();

    let url = Url::parse(&server.url()).expect("parse server url");
    let client = RpcClient::builder(url).build().expect("build client");
    let api = Nep17Api::new(Arc::new(client));

    let script_hash = UInt160::zero();
    let symbol = api.symbol(&script_hash).await.expect("symbol");
    assert_eq!(symbol, "GAS");
}

#[tokio::test]
async fn decimals_reads_integer_stack_value() {
    if !localhost_binding_permitted() {
        return;
    }

    let invoke_response = r#"{"jsonrpc":"2.0","id":1,"result":{"script":"00","state":"HALT","gasconsumed":"0","stack":[{"type":"Integer","value":"8"}]}}"#;

    let mut server = Server::new_async().await;
    let _m_invoke = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(r#""method"\s*:\s*"invokescript""#.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(invoke_response)
        .create();

    let url = Url::parse(&server.url()).expect("parse server url");
    let client = RpcClient::builder(url).build().expect("build client");
    let api = Nep17Api::new(Arc::new(client));

    let script_hash = UInt160::zero();
    let decimals = api.decimals(&script_hash).await.expect("decimals");
    assert_eq!(decimals, 8);
}

#[tokio::test]
async fn total_supply_reads_integer_stack_value() {
    if !localhost_binding_permitted() {
        return;
    }

    let invoke_response = r#"{"jsonrpc":"2.0","id":1,"result":{"script":"00","state":"HALT","gasconsumed":"0","stack":[{"type":"Integer","value":"100000000"}]}}"#;

    let mut server = Server::new_async().await;
    let _m_invoke = server
        .mock("POST", "/")
        .match_body(Matcher::Regex(r#""method"\s*:\s*"invokescript""#.into()))
        .with_status(200)
        .with_header("content-type", "application/json")
        .with_body(invoke_response)
        .create();

    let url = Url::parse(&server.url()).expect("parse server url");
    let client = RpcClient::builder(url).build().expect("build client");
    let api = Nep17Api::new(Arc::new(client));

    let script_hash = UInt160::zero();
    let total_supply = api.total_supply(&script_hash).await.expect("total supply");
    assert_eq!(total_supply, BigInt::from(100000000u64));
}

#[tokio::test]
async fn create_transfer_tx_with_from_builds_transaction() {
    if !localhost_binding_permitted() {
        return;
    }

    let key = KeyPair::from_wif("KyXwTh1hB76RRMquSvnxZrJzQx7h9nQP2PCRL38v6VDb5ip3nf1p")
        .expect("key pair");
    let from_script =
        Contract::create_signature_redeem_script(key.get_public_key_point().expect("public key"));
    let from = UInt160::from_script(&from_script);
    let to = UInt160::from_bytes(&[0x11u8; 20]).expect("to hash");
    let amount = BigInt::from(1_00000000u64);

    let mut server = Server::new_async().await;

    let url = Url::parse(&server.url()).expect("server url");
    let client = RpcClient::builder(url).build().expect("client");
    let api = Nep17Api::new(Arc::new(client));

    let transfer_script = api
        .build_transfer_script(&UInt160::zero(), &from, &to, &amount, None, true)
        .expect("transfer script");
    let transfer_script_b64 = general_purpose::STANDARD.encode(&transfer_script);
    mock_invokescript(&mut server, &transfer_script_b64, &invoke_response_empty(0));

    let balance_args = vec![serde_json::json!(from.to_string())];
    let balance_script =
        build_dynamic_call_script(&GasToken::new().hash(), "balanceOf", &balance_args);
    let balance_script_b64 = general_purpose::STANDARD.encode(balance_script);
    mock_invokescript(
        &mut server,
        &balance_script_b64,
        &invoke_response_integer(1_00000000),
    );

    mock_block_count(&mut server, 2);
    mock_calculate_network_fee(&mut server, 0);

    let tx = api
        .create_transfer_tx_with_from_and_assert(
            &UInt160::zero(),
            &from,
            &key,
            &to,
            amount,
            None,
            true,
        )
        .await
        .expect("transfer tx");

    assert_eq!(tx.script(), transfer_script.as_slice());
    assert_eq!(tx.signers().len(), 1);
    assert_eq!(tx.signers()[0].account, from);
    assert_eq!(tx.signers()[0].scopes, WitnessScope::CALLED_BY_ENTRY);
    assert_eq!(tx.network_fee(), 0);
    assert!(!tx.witnesses().is_empty());
}

#[tokio::test]
async fn create_transfer_tx_multi_sig_builds_transaction() {
    if !localhost_binding_permitted() {
        return;
    }

    let key1 = KeyPair::from_wif("KyXwTh1hB76RRMquSvnxZrJzQx7h9nQP2PCRL38v6VDb5ip3nf1p")
        .expect("key pair 1");
    let key2 = KeyPair::from_wif("KwFfNUhSDaASSAwtG7ssQM1uVX8RgX5GHWnnLfhfiQDigjioWXHH")
        .expect("key pair 2");

    let public_keys = vec![
        key1.get_public_key_point().expect("public key 1"),
        key2.get_public_key_point().expect("public key 2"),
    ];
    let m = 2usize;
    let to = UInt160::from_bytes(&[0x22u8; 20]).expect("to hash");
    let amount = BigInt::from(1_00000000u64);

    let mut server = Server::new_async().await;

    let url = Url::parse(&server.url()).expect("server url");
    let client = RpcClient::builder(url).build().expect("client");
    let api = Nep17Api::new(Arc::new(client));

    let sender = Contract::create_multi_sig_contract(m, &public_keys).script_hash();
    let transfer_script = api
        .build_transfer_script(&UInt160::zero(), &sender, &to, &amount, None, true)
        .expect("transfer script");
    let transfer_script_b64 = general_purpose::STANDARD.encode(&transfer_script);
    mock_invokescript(&mut server, &transfer_script_b64, &invoke_response_empty(0));

    let balance_args = vec![serde_json::json!(sender.to_string())];
    let balance_script =
        build_dynamic_call_script(&GasToken::new().hash(), "balanceOf", &balance_args);
    let balance_script_b64 = general_purpose::STANDARD.encode(balance_script);
    mock_invokescript(
        &mut server,
        &balance_script_b64,
        &invoke_response_integer(1_00000000),
    );

    mock_block_count(&mut server, 2);
    mock_calculate_network_fee(&mut server, 0);

    let tx = api
        .create_transfer_tx_multi_sig_with_assert(
            &UInt160::zero(),
            m,
            public_keys,
            vec![key1, key2],
            &to,
            amount,
            None,
            true,
        )
        .await
        .expect("multi-sig transfer tx");

    assert_eq!(tx.script(), transfer_script.as_slice());
    assert_eq!(tx.signers().len(), 1);
    assert_eq!(tx.signers()[0].account, sender);
    assert_eq!(tx.signers()[0].scopes, WitnessScope::CALLED_BY_ENTRY);
    assert_eq!(tx.network_fee(), 0);
    assert!(!tx.witnesses().is_empty());
}

#[tokio::test]
async fn create_transfer_tx_multi_sig_requires_enough_keys() {
    let url = Url::parse("http://localhost").expect("url");
    let client = RpcClient::builder(url).build().expect("client");
    let api = Nep17Api::new(Arc::new(client));

    let key = KeyPair::from_wif("KyXwTh1hB76RRMquSvnxZrJzQx7h9nQP2PCRL38v6VDb5ip3nf1p")
        .expect("key pair");
    let public_keys = vec![key.get_public_key_point().expect("public key")];

    let err = api
        .create_transfer_tx_multi_sig_with_assert(
            &UInt160::zero(),
            2,
            public_keys,
            vec![key],
            &UInt160::zero(),
            BigInt::from(1u8),
            None,
            true,
        )
        .await
        .expect_err("insufficient keys");

    assert_eq!(err.to_string(), "Need at least 2 KeyPairs for signing!");
}

#[test]
fn transfer_script_includes_call_flags_and_assert() {
    let url = Url::parse("http://localhost").expect("url");
    let client = RpcClient::builder(url).build().expect("build client");
    let api = Nep17Api::new(Arc::new(client));

    let script_hash = UInt160::zero();
    let from = UInt160::zero();
    let to = UInt160::zero();
    let amount = BigInt::from(1u8);
    let script = api
        .build_transfer_script(&script_hash, &from, &to, &amount, None, true)
        .expect("script");

    let mut expected = ScriptBuilder::new();
    expected.emit_opcode(OpCode::PUSHNULL);
    expected.emit_push_int(1);
    expected.emit_push(&to.to_array());
    expected.emit_push(&from.to_array());
    expected.emit_push_int(4);
    expected.emit_push_int(CallFlags::ALL.bits() as i64);
    expected.emit_push(b"transfer");
    expected.emit_push(&script_hash.to_array());
    expected
        .emit_syscall("System.Contract.Call")
        .expect("syscall");
    expected.emit_opcode(OpCode::ASSERT);

    assert_eq!(script, expected.to_array());
}

#[test]
fn transfer_script_encodes_empty_string_data() {
    let url = Url::parse("http://localhost").expect("url");
    let client = RpcClient::builder(url).build().expect("build client");
    let api = Nep17Api::new(Arc::new(client));

    let script_hash = UInt160::zero();
    let from = UInt160::zero();
    let to = UInt160::zero();
    let amount = BigInt::from(1u8);
    let script = api
        .build_transfer_script(
            &script_hash,
            &from,
            &to,
            &amount,
            Some(serde_json::Value::String(String::new())),
            true,
        )
        .expect("script");

    let mut expected = ScriptBuilder::new();
    expected.emit_push("".as_bytes());
    expected.emit_push_int(1);
    expected.emit_push(&to.to_array());
    expected.emit_push(&from.to_array());
    expected.emit_push_int(4);
    expected.emit_push_int(CallFlags::ALL.bits() as i64);
    expected.emit_push(b"transfer");
    expected.emit_push(&script_hash.to_array());
    expected
        .emit_syscall("System.Contract.Call")
        .expect("syscall");
    expected.emit_opcode(OpCode::ASSERT);

    assert_eq!(script, expected.to_array());
}

#[test]
fn transfer_script_skips_assert_when_disabled() {
    let url = Url::parse("http://localhost").expect("url");
    let client = RpcClient::builder(url).build().expect("build client");
    let api = Nep17Api::new(Arc::new(client));

    let script_hash = UInt160::zero();
    let from = UInt160::zero();
    let to = UInt160::zero();
    let amount = BigInt::from(1u8);
    let script = api
        .build_transfer_script(&script_hash, &from, &to, &amount, None, false)
        .expect("script");

    let mut expected = ScriptBuilder::new();
    expected.emit_opcode(OpCode::PUSHNULL);
    expected.emit_push_int(1);
    expected.emit_push(&to.to_array());
    expected.emit_push(&from.to_array());
    expected.emit_push_int(4);
    expected.emit_push_int(CallFlags::ALL.bits() as i64);
    expected.emit_push(b"transfer");
    expected.emit_push(&script_hash.to_array());
    expected
        .emit_syscall("System.Contract.Call")
        .expect("syscall");

    assert_eq!(script, expected.to_array());
}

#[test]
fn make_script_includes_call_flags() {
    let url = Url::parse("http://localhost").expect("url");
    let client = RpcClient::builder(url).build().expect("build client");
    let api = Nep17Api::new(Arc::new(client));

    let script_hash = UInt160::zero();
    let script = api
        .make_script(&script_hash, "balanceOf", vec![])
        .expect("script");

    let mut expected = ScriptBuilder::new();
    expected.emit_opcode(OpCode::NEWARRAY0);
    expected.emit_push_int(CallFlags::ALL.bits() as i64);
    expected.emit_push(b"balanceOf");
    expected.emit_push(&script_hash.to_array());
    expected
        .emit_syscall("System.Contract.Call")
        .expect("syscall");

    assert_eq!(script, expected.to_array());
}
