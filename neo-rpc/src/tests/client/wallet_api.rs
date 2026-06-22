use super::*;
use crate::client::test_helpers::{localhost_binding_permitted, rpc_response};
use base64::{Engine as _, engine::general_purpose};
use mockito::{Matcher, Server};
use neo_config::ProtocolSettings;
use neo_primitives::UInt256;
use neo_serialization::json::{JObject, JToken};
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::OpCode;
use regex::escape;
use reqwest::Url;
use std::fs;
use std::path::PathBuf;
use std::sync::Arc;

#[path = "wallet_api/balances.rs"]
mod balances;
#[path = "wallet_api/transfers.rs"]
mod transfers;
#[path = "wallet_api/wait.rs"]
mod wait;

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
        JToken::Array(neo_serialization::json::JArray::from(vec![JToken::Object(
            item,
        )])),
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

    sb.emit_push_int(neo_manifest::CallFlags::ALL.bits() as i64);
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
    sb.emit_push_int(neo_manifest::CallFlags::ALL.bits() as i64);
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
