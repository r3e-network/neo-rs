// Copyright (C) 2015-2025 The Neo Project.
//
// nep17_api.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::models::{RpcContractState, RpcNep17TokenInfo, RpcNep17Transfers};
use crate::{ContractClient, RpcClient, TransactionManagerFactory};
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::{Contract, ECPoint, KeyPair, Signer, Transaction};
use neo_primitives::{UInt160, WitnessScope};
use neo_vm::op_code::OpCode;
use neo_vm::{stack_item::StackItem, ScriptBuilder};
use num_bigint::BigInt;
use num_traits::cast::ToPrimitive;
use std::sync::Arc;

/// Call NEP17 methods with RPC API
/// Matches C# Nep17API
pub struct Nep17Api {
    /// Base contract client functionality
    contract_client: ContractClient,
    /// Direct access to RPC client
    rpc_client: Arc<RpcClient>,
}

impl Nep17Api {
    /// Nep17API Constructor
    /// Matches C# constructor
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self {
            contract_client: ContractClient::new(rpc_client.clone()),
            rpc_client,
        }
    }

    /// Exposes the underlying contract client for advanced scenarios.
    pub fn contract_client(&self) -> &ContractClient {
        &self.contract_client
    }

    /// Get balance of NEP17 token
    /// Matches C# BalanceOfAsync
    pub async fn balance_of(
        &self,
        script_hash: &UInt160,
        account: &UInt160,
    ) -> Result<BigInt, Box<dyn std::error::Error>> {
        let result = self
            .contract_client
            .test_invoke(
                script_hash,
                "balanceOf",
                vec![serde_json::json!(account.to_string())],
            )
            .await?;

        // Get the single stack item and convert to integer
        let stack_item = result.stack.first().ok_or("No result returned")?;

        Ok(stack_item.get_integer()?)
    }

    /// Get symbol of NEP17 token
    /// Matches C# SymbolAsync
    pub async fn symbol(
        &self,
        script_hash: &UInt160,
    ) -> Result<String, Box<dyn std::error::Error>> {
        let result = self
            .contract_client
            .test_invoke(script_hash, "symbol", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        stack_item_to_string(stack_item)
    }

    /// Get decimals of NEP17 token
    /// Matches C# DecimalsAsync
    pub async fn decimals(&self, script_hash: &UInt160) -> Result<u8, Box<dyn std::error::Error>> {
        let result = self
            .contract_client
            .test_invoke(script_hash, "decimals", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        let value = stack_item.get_integer()?;
        Ok(value.to_u8().ok_or("Invalid decimals value")?)
    }

    /// Get total supply of NEP17 token
    /// Matches C# TotalSupplyAsync
    pub async fn total_supply(
        &self,
        script_hash: &UInt160,
    ) -> Result<BigInt, Box<dyn std::error::Error>> {
        let result = self
            .contract_client
            .test_invoke(script_hash, "totalSupply", vec![])
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        Ok(stack_item.get_integer()?)
    }

    /// Get token information in one rpc call
    /// Matches C# GetTokenInfoAsync
    pub async fn get_token_info(
        &self,
        script_hash: &UInt160,
    ) -> Result<RpcNep17TokenInfo, Box<dyn std::error::Error>> {
        let contract_state = self
            .rpc_client
            .get_contract_state(&script_hash.to_string())
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        self.token_info_from_state(contract_state).await
    }

    /// Get token information for a contract hash or name.
    /// Matches C# GetTokenInfoAsync(string contractHash)
    pub async fn get_token_info_by_contract(
        &self,
        contract: &str,
    ) -> Result<RpcNep17TokenInfo, Box<dyn std::error::Error>> {
        let contract_state = self
            .rpc_client
            .get_contract_state(contract)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)?;

        self.token_info_from_state(contract_state).await
    }

    /// Get token information in one rpc call, including address info
    /// Matches C# GetTokenInfoAsync with address parameter
    pub async fn get_token_info_with_balance(
        &self,
        address: &str,
        script_hash: &UInt160,
    ) -> Result<RpcNep17TokenInfo, Box<dyn std::error::Error>> {
        let mut token_info = self.get_token_info(script_hash).await?;

        // Parse address to UInt160 using the client's address version.
        let account = if let Ok(hash) = UInt160::parse(address) {
            hash
        } else {
            WalletHelper::to_script_hash(address, self.rpc_client.protocol_settings.address_version)
                .map_err(std::io::Error::other)?
        };

        // Get balance for the address
        let balance = self.balance_of(script_hash, &account).await?;
        token_info.balance = Some(balance);

        Ok(token_info)
    }

    /// Create NEP17 token transfer transaction
    /// Matches C# CreateTransferTxAsync
    pub async fn create_transfer_tx(
        &self,
        script_hash: &UInt160,
        key: &KeyPair,
        to: &UInt160,
        amount: BigInt,
        data: Option<serde_json::Value>,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        self.create_transfer_tx_with_assert(script_hash, key, to, amount, data, true)
            .await
    }

    /// Create NEP17 token transfer transaction with specific from address
    /// Matches C# CreateTransferTxAsync with from parameter
    pub async fn create_transfer_tx_with_from(
        &self,
        script_hash: &UInt160,
        from: &UInt160,
        from_key: &KeyPair,
        to: &UInt160,
        amount: BigInt,
        data: Option<serde_json::Value>,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        self.create_transfer_tx_with_from_and_assert(
            script_hash,
            from,
            from_key,
            to,
            amount,
            data,
            true,
        )
        .await
    }

    /// Create NEP17 token transfer transaction with optional assert emission.
    pub async fn create_transfer_tx_with_assert(
        &self,
        script_hash: &UInt160,
        key: &KeyPair,
        to: &UInt160,
        amount: BigInt,
        data: Option<serde_json::Value>,
        add_assert: bool,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        let from_script = Contract::create_signature_redeem_script(key.get_public_key_point()?);
        let from = UInt160::from_script(&from_script);
        self.create_transfer_tx_with_from_and_assert(
            script_hash,
            &from,
            key,
            to,
            amount,
            data,
            add_assert,
        )
        .await
    }

    /// Create NEP17 token transfer transaction with explicit from and optional assert.
    pub async fn create_transfer_tx_with_from_and_assert(
        &self,
        script_hash: &UInt160,
        from: &UInt160,
        from_key: &KeyPair,
        to: &UInt160,
        amount: BigInt,
        data: Option<serde_json::Value>,
        add_assert: bool,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        let script =
            self.build_transfer_script(script_hash, from, to, &amount, data, add_assert)?;

        // Create signers
        let signers = vec![Signer {
            account: *from,
            scopes: WitnessScope::CALLED_BY_ENTRY,
            allowed_contracts: vec![],
            allowed_groups: vec![],
            rules: vec![],
        }];

        // Create and sign transaction
        let factory = TransactionManagerFactory::new(self.rpc_client.clone());
        let mut manager = factory.make_transaction(&script, &signers).await?;
        manager.add_signature(from_key)?;
        let transaction = manager.sign().await?;

        Ok(transaction)
    }

    /// Create NEP17 token transfer transaction from multi-sig account.
    /// Matches C# CreateTransferTxAsync with multi-sig overload.
    pub async fn create_transfer_tx_multi_sig(
        &self,
        script_hash: &UInt160,
        m: usize,
        public_keys: Vec<ECPoint>,
        from_keys: Vec<KeyPair>,
        to: &UInt160,
        amount: BigInt,
        data: Option<serde_json::Value>,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        self.create_transfer_tx_multi_sig_with_assert(
            script_hash,
            m,
            public_keys,
            from_keys,
            to,
            amount,
            data,
            true,
        )
        .await
    }

    /// Create NEP17 token transfer transaction from multi-sig account with optional assert.
    pub async fn create_transfer_tx_multi_sig_with_assert(
        &self,
        script_hash: &UInt160,
        m: usize,
        public_keys: Vec<ECPoint>,
        from_keys: Vec<KeyPair>,
        to: &UInt160,
        amount: BigInt,
        data: Option<serde_json::Value>,
        add_assert: bool,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        if m > from_keys.len() {
            return Err(format!("Need at least {m} KeyPairs for signing!").into());
        }

        let sender = Contract::create_multi_sig_contract(m, &public_keys).script_hash();
        let script =
            self.build_transfer_script(script_hash, &sender, to, &amount, data, add_assert)?;

        let signers = vec![Signer {
            account: sender,
            scopes: WitnessScope::CALLED_BY_ENTRY,
            allowed_contracts: vec![],
            allowed_groups: vec![],
            rules: vec![],
        }];

        let factory = TransactionManagerFactory::new(self.rpc_client.clone());
        let mut manager = factory.make_transaction(&script, &signers).await?;
        manager.add_multi_sig_with_keys(from_keys, m, public_keys)?;
        let transaction = manager.sign().await?;

        Ok(transaction)
    }

    /// Get NEP17 token transfers
    /// Matches C# GetNep17TransfersAsync
    pub async fn get_nep17_transfers(
        &self,
        address: &str,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> Result<RpcNep17Transfers, Box<dyn std::error::Error>> {
        self.rpc_client
            .get_nep17_transfers(address, start_time, end_time)
            .await
            .map_err(|e| Box::new(e) as Box<dyn std::error::Error>)
    }

    /// Get NEP17 token balances for an address
    /// Matches C# GetNep17BalancesAsync
    pub async fn get_nep17_balances(
        &self,
        address: &str,
    ) -> Result<Vec<RpcNep17TokenInfo>, Box<dyn std::error::Error>> {
        let balances = self.rpc_client.get_nep17_balances(address).await?;

        // Convert balances to token info
        let mut token_infos = Vec::new();
        for balance in balances.balances {
            let mut info = self.get_token_info(&balance.asset_hash).await?;
            info.balance = Some(balance.amount);
            info.last_updated_block = Some(balance.last_updated_block);
            token_infos.push(info);
        }

        Ok(token_infos)
    }

    // Helper methods
    fn build_transfer_script(
        &self,
        script_hash: &UInt160,
        from: &UInt160,
        to: &UInt160,
        amount: &BigInt,
        data: Option<serde_json::Value>,
        add_assert: bool,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut sb = ScriptBuilder::new();

        // Emit transfer parameters in reverse order.
        if let Some(d) = data {
            self.emit_argument(&mut sb, &d)?;
        } else {
            sb.emit_opcode(OpCode::PUSHNULL);
        }
        sb.emit_push_int(amount.to_i64().ok_or("Amount too large")?);
        sb.emit_push(&to.to_array());
        sb.emit_push(&from.to_array());
        sb.emit_push_int(4);
        sb.emit_push_int(CallFlags::ALL.bits() as i64);
        sb.emit_push(b"transfer");
        sb.emit_push(&script_hash.to_array());
        sb.emit_syscall("System.Contract.Call")?;
        if add_assert {
            sb.emit_opcode(OpCode::ASSERT);
        }

        Ok(sb.to_array())
    }

    async fn token_info_from_state(
        &self,
        contract_state: RpcContractState,
    ) -> Result<RpcNep17TokenInfo, Box<dyn std::error::Error>> {
        let contract_hash = &contract_state.contract_state.hash;
        let name = contract_state.contract_state.manifest.name.clone();

        // Build script to get all token info at once.
        let mut script = Vec::new();
        script.extend(self.make_script(contract_hash, "symbol", vec![])?);
        script.extend(self.make_script(contract_hash, "decimals", vec![])?);
        script.extend(self.make_script(contract_hash, "totalSupply", vec![])?);

        let result = self.rpc_client.invoke_script(&script).await?;
        let stack = &result.stack;

        Ok(RpcNep17TokenInfo {
            name,
            symbol: stack
                .first()
                .and_then(|s| stack_item_to_string(s).ok())
                .unwrap_or_default(),
            decimals: stack
                .get(1)
                .and_then(|s| s.get_integer().ok())
                .and_then(|i| i.to_u8())
                .unwrap_or(0),
            total_supply: stack
                .get(2)
                .and_then(|s| s.get_integer().ok())
                .unwrap_or_else(|| BigInt::from(0)),
            balance: None,
            last_updated_block: None,
        })
    }

    fn make_script(
        &self,
        script_hash: &UInt160,
        operation: &str,
        args: Vec<serde_json::Value>,
    ) -> Result<Vec<u8>, Box<dyn std::error::Error>> {
        let mut sb = ScriptBuilder::new();

        if args.is_empty() {
            sb.emit_opcode(OpCode::NEWARRAY0);
        } else {
            for arg in args.iter().rev() {
                self.emit_argument(&mut sb, arg)?;
            }
            sb.emit_push_int(args.len() as i64);
            sb.emit_pack();
        }

        sb.emit_push_int(CallFlags::ALL.bits() as i64);
        sb.emit_push(operation.as_bytes());
        sb.emit_push(&script_hash.to_array());
        sb.emit_syscall("System.Contract.Call")?;

        Ok(sb.to_array())
    }

    fn emit_argument(
        &self,
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
                    self.emit_argument(sb, item)?;
                }
                sb.emit_push_int(arr.len() as i64);
                sb.emit_pack();
                Ok(())
            }
            _ => Err("Unsupported argument type".into()),
        }
    }
}

fn stack_item_to_string(item: &StackItem) -> Result<String, Box<dyn std::error::Error>> {
    match item {
        StackItem::ByteString(bytes) => Ok(String::from_utf8(bytes.clone())?),
        StackItem::Buffer(buffer) => Ok(String::from_utf8(buffer.data())?),
        StackItem::Integer(int) => Ok(int.to_string()),
        StackItem::Boolean(b) => Ok(b.to_string()),
        _ => Err("Unsupported stack item for string conversion".into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose, Engine as _};
    use mockito::{Matcher, Server};
    use neo_core::smart_contract::native::GasToken;
    use neo_core::NativeContract;
    use neo_json::{JArray, JObject, JToken};
    use neo_vm::op_code::OpCode;
    use neo_vm::ScriptBuilder;
    use regex::escape;
    use reqwest::Url;
    use std::fs;
    use std::path::PathBuf;
    use std::sync::Arc;

    fn localhost_binding_permitted() -> bool {
        std::net::TcpListener::bind("127.0.0.1:0").is_ok()
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

    fn invoke_response_empty(gas_consumed: i64) -> String {
        invoke_response(Vec::new(), gas_consumed)
    }

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

    fn load_contract_state_case(manifest_name: &str) -> (String, String) {
        let mut path = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        path.push("..");
        path.push("neo_csharp");
        path.push("node");
        path.push("tests");
        path.push("Neo.Network.RPC.Tests");
        path.push("RpcTestCases.json");
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
                neo_core::smart_contract::native::NativeRegistry::new()
                    .get_by_name(&contract)
                    .map(|native| native.hash().to_string())
                    .unwrap_or(contract)
            };

            return (contract, response.to_string());
        }

        panic!("RpcTestCases.json missing contract state for {manifest_name}");
    }

    #[tokio::test]
    async fn token_info_uses_contract_state_name() {
        if !localhost_binding_permitted() {
            return;
        }

        let (contract_hash, contract_state_response) = load_contract_state_case("GasToken");
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

        let (_contract_hash, contract_state_response) = load_contract_state_case("GasToken");
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
        let (contract_hash, contract_state_response) = load_contract_state_case("GasToken");
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
        let from_script = Contract::create_signature_redeem_script(
            key.get_public_key_point().expect("public key"),
        );
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
}
