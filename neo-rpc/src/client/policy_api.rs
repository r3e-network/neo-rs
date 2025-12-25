// Copyright (C) 2015-2025 The Neo Project.
//
// policy_api.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::{ContractClient, RpcClient};
use neo_core::smart_contract::native::PolicyContract;
use neo_core::NativeContract;
use neo_primitives::UInt160;
use num_traits::cast::ToPrimitive;
use std::sync::Arc;

/// Get Policy info by RPC API
/// Matches C# PolicyAPI
pub struct PolicyApi {
    /// Base contract client functionality
    contract_client: ContractClient,
    /// Policy contract script hash
    script_hash: UInt160,
}

impl PolicyApi {
    /// PolicyAPI Constructor
    /// Matches C# constructor
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self {
            contract_client: ContractClient::new(rpc_client),
            script_hash: PolicyContract::new().hash(),
        }
    }

    /// Get Fee Factor
    /// Matches C# GetExecFeeFactorAsync
    pub async fn get_exec_fee_factor(&self) -> Result<u32, Box<dyn std::error::Error>> {
        let result = self
            .contract_client
            .test_invoke(&self.script_hash, "getExecFeeFactor", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        let value = stack_item.get_integer()?;
        Ok(value.to_u32().ok_or("Invalid fee factor value")?)
    }

    /// Get Storage Price
    /// Matches C# GetStoragePriceAsync
    pub async fn get_storage_price(&self) -> Result<u32, Box<dyn std::error::Error>> {
        let result = self
            .contract_client
            .test_invoke(&self.script_hash, "getStoragePrice", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        let value = stack_item.get_integer()?;
        Ok(value.to_u32().ok_or("Invalid storage price value")?)
    }

    /// Get Network Fee Per Byte
    /// Matches C# GetFeePerByteAsync
    pub async fn get_fee_per_byte(&self) -> Result<i64, Box<dyn std::error::Error>> {
        let result = self
            .contract_client
            .test_invoke(&self.script_hash, "getFeePerByte", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        let value = stack_item.get_integer()?;
        Ok(value.to_i64().ok_or("Invalid fee per byte value")?)
    }

    /// Get Policy Blocked Accounts
    /// Matches C# IsBlockedAsync
    pub async fn is_blocked(&self, account: &UInt160) -> Result<bool, Box<dyn std::error::Error>> {
        let result = self
            .contract_client
            .test_invoke(
                &self.script_hash,
                "isBlocked",
                vec![serde_json::json!(account.to_string())],
            )
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        Ok(stack_item.get_boolean()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose, Engine as _};
    use mockito::{Matcher, Server};
    use neo_json::{JArray, JObject, JToken};
    use neo_vm::op_code::OpCode;
    use neo_vm::ScriptBuilder;
    use regex::escape;
    use reqwest::Url;
    use std::net::TcpListener;
    use std::sync::Arc;

    fn localhost_binding_permitted() -> bool {
        TcpListener::bind("127.0.0.1:0").is_ok()
    }

    fn rpc_response(result: JToken) -> String {
        let mut response = JObject::new();
        response.insert("jsonrpc".to_string(), JToken::String("2.0".to_string()));
        response.insert("id".to_string(), JToken::Number(1.0));
        response.insert("result".to_string(), result);
        JToken::Object(response).to_string()
    }

    fn invoke_response(stack_item: JObject) -> String {
        let mut result = JObject::new();
        result.insert("script".to_string(), JToken::String("00".to_string()));
        result.insert("state".to_string(), JToken::String("HALT".to_string()));
        result.insert(
            "gasconsumed".to_string(),
            JToken::String("0".to_string()),
        );
        result.insert(
            "stack".to_string(),
            JToken::Array(JArray::from(vec![JToken::Object(stack_item)])),
        );
        rpc_response(JToken::Object(result))
    }

    fn integer_stack_item(value: i64) -> JObject {
        let mut item = JObject::new();
        item.insert("type".to_string(), JToken::String("Integer".to_string()));
        item.insert("value".to_string(), JToken::String(value.to_string()));
        item
    }

    fn boolean_stack_item(value: bool) -> JObject {
        let mut item = JObject::new();
        item.insert("type".to_string(), JToken::String("Boolean".to_string()));
        item.insert("value".to_string(), JToken::Boolean(value));
        item
    }

    fn emit_argument(sb: &mut ScriptBuilder, arg: &serde_json::Value) -> Result<(), String> {
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
                    Err("Invalid number format".to_string())
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
            _ => Err("Unsupported argument type".to_string()),
        }
    }

    fn build_policy_script(operation: &str, args: &[serde_json::Value]) -> Vec<u8> {
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

        sb.emit_push_int(neo_core::smart_contract::call_flags::CallFlags::ALL.bits() as i64);
        sb.emit_push(operation.as_bytes());
        sb.emit_push(&PolicyContract::new().hash().to_array());
        sb.emit_syscall("System.Contract.Call")
            .expect("syscall");

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

    #[tokio::test]
    async fn policy_api_get_exec_fee_factor_reads_integer() {
        if !localhost_binding_permitted() {
            return;
        }

        let mut server = Server::new_async().await;
        let script = build_policy_script("getExecFeeFactor", &[]);
        let script_b64 = general_purpose::STANDARD.encode(script);
        let response = invoke_response(integer_stack_item(30));
        mock_invokescript(&mut server, &script_b64, &response);

        let url = Url::parse(&server.url()).expect("server url");
        let client = RpcClient::builder(url).build().expect("client");
        let api = PolicyApi::new(Arc::new(client));

        let result = api.get_exec_fee_factor().await.expect("fee factor");
        assert_eq!(result, 30);
    }

    #[tokio::test]
    async fn policy_api_get_storage_price_reads_integer() {
        if !localhost_binding_permitted() {
            return;
        }

        let mut server = Server::new_async().await;
        let script = build_policy_script("getStoragePrice", &[]);
        let script_b64 = general_purpose::STANDARD.encode(script);
        let response = invoke_response(integer_stack_item(100_000));
        mock_invokescript(&mut server, &script_b64, &response);

        let url = Url::parse(&server.url()).expect("server url");
        let client = RpcClient::builder(url).build().expect("client");
        let api = PolicyApi::new(Arc::new(client));

        let result = api.get_storage_price().await.expect("storage price");
        assert_eq!(result, 100_000);
    }

    #[tokio::test]
    async fn policy_api_get_fee_per_byte_reads_integer() {
        if !localhost_binding_permitted() {
            return;
        }

        let mut server = Server::new_async().await;
        let script = build_policy_script("getFeePerByte", &[]);
        let script_b64 = general_purpose::STANDARD.encode(script);
        let response = invoke_response(integer_stack_item(1_000));
        mock_invokescript(&mut server, &script_b64, &response);

        let url = Url::parse(&server.url()).expect("server url");
        let client = RpcClient::builder(url).build().expect("client");
        let api = PolicyApi::new(Arc::new(client));

        let result = api.get_fee_per_byte().await.expect("fee per byte");
        assert_eq!(result, 1_000);
    }

    #[tokio::test]
    async fn policy_api_is_blocked_reads_boolean() {
        if !localhost_binding_permitted() {
            return;
        }

        let mut server = Server::new_async().await;
        let account = UInt160::zero();
        let args = vec![serde_json::json!(account.to_string())];
        let script = build_policy_script("isBlocked", &args);
        let script_b64 = general_purpose::STANDARD.encode(script);
        let response = invoke_response(boolean_stack_item(true));
        mock_invokescript(&mut server, &script_b64, &response);

        let url = Url::parse(&server.url()).expect("server url");
        let client = RpcClient::builder(url).build().expect("client");
        let api = PolicyApi::new(Arc::new(client));

        let result = api.is_blocked(&account).await.expect("is blocked");
        assert!(result);
    }
}
