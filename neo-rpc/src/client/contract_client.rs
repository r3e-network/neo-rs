use super::contract_script::{build_dynamic_call_script, emit_contract_call};
use super::models::RpcInvokeResult;
use crate::{RpcClient, RpcError};
use neo_manifest::ContractManifest;
use neo_native_contracts::ContractManagement;
use neo_payloads::{Signer, Transaction};
use neo_primitives::UInt160;
use neo_primitives::{CallFlags, WitnessScope};
use neo_vm::script_builder::ScriptBuilder;
use neo_wallets::KeyPair;
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
    ) -> Result<RpcInvokeResult, RpcError> {
        // Create script using script builder
        let script = build_dynamic_call_script(script_hash, operation, &args, CallFlags::ALL)?;

        // Call RPC invoke script method
        self.rpc_client
            .invoke_script(&script)
            .await
            .map_err(Into::into)
    }

    /// Deploy Contract, return signed transaction
    /// Matches C# `CreateDeployContractTxAsync`
    pub async fn create_deploy_contract_tx(
        &self,
        nef_file: &[u8],
        manifest: &ContractManifest,
        key: &KeyPair,
    ) -> Result<Transaction, RpcError> {
        let script = Self::build_deploy_contract_script(nef_file, manifest, CallFlags::ALL)?;

        let sender = key.get_script_hash();
        let signers = vec![Signer::new(sender, WitnessScope::CALLED_BY_ENTRY)];

        let mut manager = crate::TransactionManagerFactory::new(self.rpc_client.clone())
            .make_transaction(&script, &signers)
            .await?;
        manager.add_signature(key)?;
        manager.sign().await
    }

    #[cfg(test)]
    fn build_dynamic_call_script(
        script_hash: &UInt160,
        method: &str,
        args: &[serde_json::Value],
        call_flags: CallFlags,
    ) -> Result<Vec<u8>, RpcError> {
        build_dynamic_call_script(script_hash, method, args, call_flags)
    }

    fn build_deploy_contract_script(
        nef_file: &[u8],
        manifest: &ContractManifest,
        call_flags: CallFlags,
    ) -> Result<Vec<u8>, RpcError> {
        let manifest_json = manifest.to_json()?.to_string();

        let mut sb = ScriptBuilder::new();
        // C# parity: ScriptBuilderExtensions.EmitDynamicCall(ContractManagement.Hash, "deploy", nef, manifestJson)
        sb.emit_push(manifest_json.as_bytes());
        sb.emit_push(nef_file);
        sb.emit_push_int(2);
        sb.emit_pack();
        emit_contract_call(
            &mut sb,
            &ContractManagement::script_hash(),
            "deploy",
            call_flags,
        )?;

        Ok(sb.to_array())
    }
}

// NOTE: Script byte layout parity is covered by the VM/native-contract
// compatibility tests, so this optional client module only checks RPC assembly.

#[cfg(test)]
mod tests {
    use super::*;
    use crate::client::test_helpers::{localhost_binding_permitted, rpc_response};
    use base64::{Engine as _, engine::general_purpose};
    use mockito::{Matcher, Server};
    use neo_config::ProtocolSettings;
    use neo_manifest::ContractManifest;
    use neo_native_contracts::GasToken;
    use neo_primitives::UInt160;
    use neo_serialization::json::{JArray, JObject, JToken};
    use neo_vm::script_builder::ScriptBuilder;
    use neo_vm_rs::{OpCode, StackValue};
    use neo_wallets::KeyPair;
    use num_bigint::BigInt;
    use regex::escape;
    use reqwest::Url;
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

        let value =
            crate::RpcUtility::stack_value_to_bigint(result.stack.first().expect("stack item"))
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

        let StackValue::Map(_, map) = &result.stack[0] else {
            panic!("expected map");
        };
        assert_eq!(map.len(), 1);
        let (key, value) = map.first().expect("entry");
        assert_eq!(key.as_bytes().expect("key bytes"), b"key");
        assert_eq!(
            crate::RpcUtility::stack_value_to_bigint(value).expect("value int"),
            BigInt::from(42)
        );

        let StackValue::Struct(_, structure) = &result.stack[1] else {
            panic!("expected struct");
        };
        assert_eq!(structure.len(), 2);
        assert_eq!(
            crate::RpcUtility::stack_value_to_bigint(&structure[0]).unwrap(),
            BigInt::from(1)
        );
        assert_eq!(
            crate::RpcUtility::stack_value_to_bigint(&structure[1]).unwrap(),
            BigInt::from(2)
        );
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
