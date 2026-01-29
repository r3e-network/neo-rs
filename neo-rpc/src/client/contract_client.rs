// Copyright (C) 2015-2025 The Neo Project.
//
// contract_client.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::models::RpcInvokeResult;
use crate::RpcClient;
use neo_core::smart_contract::native::ContractManagement;
use neo_core::{
    smart_contract::call_flags::CallFlags, ContractManifest, KeyPair, Signer, Transaction,
    WitnessScope,
};
use neo_primitives::UInt160;
use neo_vm::op_code::OpCode;
use neo_vm::ScriptBuilder;
use std::sync::Arc;

/// Contract related operations through RPC API
/// Matches C# `ContractClient`
pub struct ContractClient {
    /// The RPC client instance
    rpc_client: Arc<RpcClient>,
}

impl ContractClient {
    /// `ContractClient` Constructor
    /// Matches C# constructor
    #[must_use] 
    pub const fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self { rpc_client }
    }

    /// Use RPC method to test invoke operation
    /// Matches C# `TestInvokeAsync`
    pub async fn test_invoke(
        &self,
        script_hash: &UInt160,
        operation: &str,
        args: Vec<serde_json::Value>,
    ) -> Result<RpcInvokeResult, Box<dyn std::error::Error>> {
        // Create script using script builder
        let script =
            Self::build_dynamic_call_script(script_hash, operation, &args, CallFlags::ALL)?;

        // Call RPC invoke script method
        self.rpc_client
            .invoke_script(&script)
            .await
            .map_err(|err| Box::new(err) as Box<dyn std::error::Error>)
    }

    /// Deploy Contract, return signed transaction
    /// Matches C# `CreateDeployContractTxAsync`
    pub async fn create_deploy_contract_tx(
        &self,
        nef_file: &[u8],
        manifest: &ContractManifest,
        key: &KeyPair,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        let script = Self::build_deploy_contract_script(nef_file, manifest, CallFlags::ALL)?;

        let sender = key.get_script_hash();
        let signers = vec![Signer::new(sender, WitnessScope::CALLED_BY_ENTRY)];

        let mut manager = crate::TransactionManagerFactory::new(self.rpc_client.clone())
            .make_transaction(&script, &signers)
            .await?;
        manager.add_signature(key)?;
        manager.sign().await
    }

    fn build_dynamic_call_script(
        script_hash: &UInt160,
        method: &str,
        args: &[serde_json::Value],
        call_flags: CallFlags,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut sb = ScriptBuilder::new();

        // C# parity: ScriptBuilderExtensions.EmitDynamicCall(scriptHash, method, CallFlags.All, args)
        if args.is_empty() {
            sb.emit_opcode(OpCode::NEWARRAY0);
        } else {
            // CreateArray(args): push elements in reverse order, push count, PACK
            for arg in args.iter().rev() {
                Self::emit_argument(&mut sb, arg)?;
            }
            sb.emit_push_int(args.len() as i64);
            sb.emit_pack();
        }

        // EmitPush(flags), EmitPush(method), EmitPush(scriptHash), SYSCALL System.Contract.Call
        sb.emit_push_int(i64::from(call_flags.bits()));
        sb.emit_push(method.as_bytes());
        sb.emit_push(&script_hash.to_array());
        sb.emit_syscall("System.Contract.Call")?;

        Ok(sb.to_array())
    }

    fn build_deploy_contract_script(
        nef_file: &[u8],
        manifest: &ContractManifest,
        call_flags: CallFlags,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let manifest_json = manifest.to_json()?.to_string();

        let mut sb = ScriptBuilder::new();
        // C# parity: ScriptBuilderExtensions.EmitDynamicCall(ContractManagement.Hash, "deploy", nef, manifestJson)
        // CreateArray(args)
        sb.emit_push(manifest_json.as_bytes());
        sb.emit_push(nef_file);
        sb.emit_push_int(2);
        sb.emit_pack();
        // EmitPush(flags)
        sb.emit_push_int(i64::from(call_flags.bits()));
        // EmitPush(method)
        sb.emit_push(b"deploy");
        // EmitPush(scriptHash)
        sb.emit_push(&ContractManagement::contract_hash().to_array());
        // Syscall
        sb.emit_syscall("System.Contract.Call")?;

        Ok(sb.to_array())
    }

    /// Helper to emit argument based on type
    fn emit_argument(
        sb: &mut ScriptBuilder,
        arg: &serde_json::Value,
    ) -> Result<(), Box<dyn std::error::Error>> {
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
                // C# CreateArray pushes in reverse order.
                for item in arr.iter().rev() {
                    Self::emit_argument(sb, item)?;
                }
                sb.emit_push_int(arr.len() as i64);
                sb.emit_pack();
                Ok(())
            }
            _ => Err("Unsupported argument type".into()),
        }
    }
}

// NOTE: Script byte layout parity tests live in `neo-vm/tests/*` so they run in
// CI without requiring the optional `neo-rpc/client` feature.

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose, Engine as _};
    use mockito::{Matcher, Server};
    use neo_config::ProtocolSettings;
    use neo_core::smart_contract::native::{GasToken, NativeContract};
    use neo_core::{ContractManifest, KeyPair};
    use neo_json::{JArray, JObject, JToken};
    use neo_primitives::UInt160;
    use neo_vm::{OpCode, ScriptBuilder};
    use num_bigint::BigInt;
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

    fn invoke_response_bytestring(value_b64: &str) -> String {
        let mut item = JObject::new();
        item.insert("type".to_string(), JToken::String("ByteString".to_string()));
        item.insert("value".to_string(), JToken::String(value_b64.to_string()));
        invoke_response(vec![JToken::Object(item)], 0)
    }

    fn invoke_response_empty(gas_consumed: i64) -> String {
        invoke_response(Vec::new(), gas_consumed)
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

    #[test]
    fn dynamic_call_script_matches_emit_dynamic_call_shape() {
        let script_hash = UInt160::zero();
        let script = ContractClient::build_dynamic_call_script(
            &script_hash,
            "balanceOf",
            &[],
            CallFlags::ALL,
        )
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

    #[test]
    fn dynamic_call_script_packs_arguments() {
        let script_hash = UInt160::zero();
        let args = vec![serde_json::json!(1), serde_json::json!("hi")];
        let script = ContractClient::build_dynamic_call_script(
            &script_hash,
            "transfer",
            &args,
            CallFlags::ALL,
        )
        .expect("script");

        let mut expected = ScriptBuilder::new();
        expected.emit_push("hi".as_bytes());
        expected.emit_push_int(1);
        expected.emit_push_int(2);
        expected.emit_pack();
        expected.emit_push_int(CallFlags::ALL.bits() as i64);
        expected.emit_push(b"transfer");
        expected.emit_push(&script_hash.to_array());
        expected
            .emit_syscall("System.Contract.Call")
            .expect("syscall");

        assert_eq!(script, expected.to_array());
    }

    #[tokio::test]
    async fn test_invoke_reads_integer_from_bytestring_stack_item() {
        if !localhost_binding_permitted() {
            return;
        }

        let script_hash = GasToken::new().hash();
        let account = UInt160::zero();
        let args = vec![serde_json::json!(account.to_string())];
        let script = ContractClient::build_dynamic_call_script(
            &script_hash,
            "balanceOf",
            &args,
            CallFlags::ALL,
        )
        .expect("script");
        let script_b64 = general_purpose::STANDARD.encode(script);

        let bytes = [0x00u8, 0xe0, 0x57, 0xeb, 0x48, 0x1b];
        let bytes_b64 = general_purpose::STANDARD.encode(bytes);
        let response = invoke_response_bytestring(&bytes_b64);

        let mut server = Server::new_async().await;
        mock_invokescript(&mut server, &script_b64, &response);

        let url = Url::parse(&server.url()).expect("server url");
        let client = RpcClient::builder(url).build().expect("client");
        let contract_client = ContractClient::new(Arc::new(client));

        let result = contract_client
            .test_invoke(&script_hash, "balanceOf", args)
            .await
            .expect("invoke");

        let value = result
            .stack
            .first()
            .expect("stack item")
            .get_integer()
            .expect("integer");
        assert_eq!(value, BigInt::from(30_000_000_000_000i64));
    }

    #[tokio::test]
    async fn test_invoke_parses_map_and_struct_stack_items() {
        if !localhost_binding_permitted() {
            return;
        }

        let script_hash = UInt160::zero();
        let args = vec![serde_json::json!(1)];
        let script =
            ContractClient::build_dynamic_call_script(&script_hash, "get", &args, CallFlags::ALL)
                .expect("script");
        let script_b64 = general_purpose::STANDARD.encode(script);

        let key_b64 = general_purpose::STANDARD.encode("key".as_bytes());
        let mut key_obj = JObject::new();
        key_obj.insert("type".to_string(), JToken::String("ByteString".to_string()));
        key_obj.insert("value".to_string(), JToken::String(key_b64));

        let mut value_obj = JObject::new();
        value_obj.insert("type".to_string(), JToken::String("Integer".to_string()));
        value_obj.insert("value".to_string(), JToken::String("42".to_string()));

        let mut map_entry = JObject::new();
        map_entry.insert("key".to_string(), JToken::Object(key_obj));
        map_entry.insert("value".to_string(), JToken::Object(value_obj));

        let mut map_obj = JObject::new();
        map_obj.insert("type".to_string(), JToken::String("Map".to_string()));
        map_obj.insert(
            "value".to_string(),
            JToken::Array(JArray::from(vec![JToken::Object(map_entry)])),
        );

        let mut struct_first = JObject::new();
        struct_first.insert("type".to_string(), JToken::String("Integer".to_string()));
        struct_first.insert("value".to_string(), JToken::String("1".to_string()));

        let mut struct_second = JObject::new();
        struct_second.insert("type".to_string(), JToken::String("Integer".to_string()));
        struct_second.insert("value".to_string(), JToken::String("2".to_string()));

        let mut struct_obj = JObject::new();
        struct_obj.insert("type".to_string(), JToken::String("Struct".to_string()));
        struct_obj.insert(
            "value".to_string(),
            JToken::Array(JArray::from(vec![
                JToken::Object(struct_first),
                JToken::Object(struct_second),
            ])),
        );

        let response =
            invoke_response(vec![JToken::Object(map_obj), JToken::Object(struct_obj)], 0);

        let mut server = Server::new_async().await;
        mock_invokescript(&mut server, &script_b64, &response);

        let url = Url::parse(&server.url()).expect("server url");
        let client = RpcClient::builder(url).build().expect("client");
        let contract_client = ContractClient::new(Arc::new(client));

        let result = contract_client
            .test_invoke(&script_hash, "get", args)
            .await
            .expect("invoke");

        let map = result.stack[0].as_map().expect("map");
        assert_eq!(map.len(), 1);
        let (key, value) = map.iter().next().expect("entry");
        assert_eq!(key.as_bytes().expect("key bytes"), b"key");
        assert_eq!(value.get_integer().expect("value int"), BigInt::from(42));

        let structure = result.stack[1].as_array().expect("struct");
        assert_eq!(structure.len(), 2);
        assert_eq!(structure[0].get_integer().unwrap(), BigInt::from(1));
        assert_eq!(structure[1].get_integer().unwrap(), BigInt::from(2));
    }

    #[tokio::test]
    async fn create_deploy_contract_tx_builds_signed_transaction() {
        if !localhost_binding_permitted() {
            return;
        }

        let settings = ProtocolSettings::default_settings();
        let key = KeyPair::from_wif("KyXwTh1hB76RRMquSvnxZrJzQx7h9nQP2PCRL38v6VDb5ip3nf1p")
            .expect("key pair");
        let sender = key.get_script_hash();
        let manifest = ContractManifest::new("TestContract".to_string());
        let nef = vec![1u8];

        let deploy_script =
            ContractClient::build_deploy_contract_script(&nef, &manifest, CallFlags::ALL)
                .expect("deploy script");
        let deploy_script_b64 = general_purpose::STANDARD.encode(&deploy_script);

        let balance_args = vec![serde_json::json!(sender.to_string())];
        let balance_script = ContractClient::build_dynamic_call_script(
            &GasToken::new().hash(),
            "balanceOf",
            &balance_args,
            CallFlags::ALL,
        )
        .expect("balance script");
        let balance_script_b64 = general_purpose::STANDARD.encode(&balance_script);

        let mut server = Server::new_async().await;
        mock_invokescript(&mut server, &deploy_script_b64, &invoke_response_empty(0));
        mock_invokescript(
            &mut server,
            &balance_script_b64,
            &invoke_response_integer(1_00000000),
        );
        mock_block_count(&mut server, 2);
        mock_calculate_network_fee(&mut server, 0);

        let url = Url::parse(&server.url()).expect("server url");
        let client = RpcClient::builder(url)
            .protocol_settings(settings)
            .build()
            .expect("client");
        let contract_client = ContractClient::new(Arc::new(client));

        let tx = contract_client
            .create_deploy_contract_tx(&nef, &manifest, &key)
            .await
            .expect("deploy tx");

        assert_eq!(tx.script(), deploy_script.as_slice());
        assert_eq!(tx.signers().len(), 1);
        assert_eq!(tx.signers()[0].account, sender);
        assert_eq!(tx.signers()[0].scopes, WitnessScope::CALLED_BY_ENTRY);
        assert!(!tx.witnesses().is_empty());
    }
}
