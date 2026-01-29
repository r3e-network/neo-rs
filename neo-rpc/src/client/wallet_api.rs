// Copyright (C) 2015-2025 The Neo Project.
//
// wallet_api.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::models::RpcTransaction;
use crate::{Nep17Api, RpcClient, RpcUtility};
use neo_core::big_decimal::BigDecimal;
use neo_core::smart_contract::native::{GasToken, NeoToken};
use neo_core::wallets::helper::Helper as WalletHelper;
use neo_core::{Contract, ECPoint, KeyPair, NativeContract, Transaction};
use neo_primitives::UInt160;
use num_bigint::BigInt;
use num_traits::cast::ToPrimitive;
use std::sync::Arc;

/// Wallet Common APIs
/// Matches C# `WalletAPI`
pub struct WalletApi {
    /// The RPC client instance
    rpc_client: Arc<RpcClient>,
    /// NEP17 API for token operations
    nep17_api: Nep17Api,
}

impl WalletApi {
    /// `WalletAPI` Constructor
    /// Matches C# constructor
    #[must_use]
    pub fn new(rpc_client: Arc<RpcClient>) -> Self {
        Self {
            nep17_api: Nep17Api::new(rpc_client.clone()),
            rpc_client,
        }
    }

    /// Get unclaimed gas with address, scripthash or public key string
    /// Matches C# `GetUnclaimedGasAsync` with string parameter
    pub async fn get_unclaimed_gas(
        &self,
        account: &str,
    ) -> Result<f64, Box<dyn std::error::Error>> {
        let account_hash =
            RpcUtility::get_script_hash(account, &self.rpc_client.protocol_settings)?;
        self.get_unclaimed_gas_from_hash(&account_hash).await
    }

    /// Get unclaimed gas
    /// Matches C# `GetUnclaimedGasAsync` with `UInt160` parameter
    pub async fn get_unclaimed_gas_from_hash(
        &self,
        account: &UInt160,
    ) -> Result<f64, Box<dyn std::error::Error>> {
        let script_hash = neo_hash();
        let block_count = self.rpc_client.get_block_count().await?;

        let result = self
            .nep17_api
            .contract_client()
            .test_invoke(
                &script_hash,
                "unclaimedGas",
                vec![
                    serde_json::json!(account.to_string()),
                    serde_json::json!(block_count - 1),
                ],
            )
            .await?;

        let stack_item = result.stack.first().ok_or("No result returned")?;

        let balance = stack_item.get_integer()?;
        let gas_factor = gas_factor();

        Ok(balance.to_f64().unwrap_or(0.0) / gas_factor as f64)
    }

    /// Get Neo Balance
    /// Matches C# `GetNeoBalanceAsync`
    pub async fn get_neo_balance(&self, account: &str) -> Result<u32, Box<dyn std::error::Error>> {
        let balance = self
            .get_token_balance(&neo_hash().to_string(), account)
            .await?;
        Ok(balance.to_u32().ok_or("Invalid NEO balance")?)
    }

    /// Get Gas Balance
    /// Matches C# `GetGasBalanceAsync`
    pub async fn get_gas_balance(&self, account: &str) -> Result<f64, Box<dyn std::error::Error>> {
        let balance = self
            .get_token_balance(&gas_hash().to_string(), account)
            .await?;
        let gas_factor = gas_factor();
        Ok(balance.to_f64().unwrap_or(0.0) / gas_factor as f64)
    }

    /// Get token balance with string parameters
    /// Matches C# `GetTokenBalanceAsync`
    pub async fn get_token_balance(
        &self,
        token_hash: &str,
        account: &str,
    ) -> Result<BigInt, Box<dyn std::error::Error>> {
        let token_script_hash =
            RpcUtility::get_script_hash(token_hash, &self.rpc_client.protocol_settings)?;
        let account_hash =
            RpcUtility::get_script_hash(account, &self.rpc_client.protocol_settings)?;

        self.nep17_api
            .balance_of(&token_script_hash, &account_hash)
            .await
    }

    /// Claim GAS from NEO
    /// Matches C# `ClaimGasAsync`
    pub async fn claim_gas(
        &self,
        key: &KeyPair,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        self.claim_gas_with_assert(key, true).await
    }

    /// Claim GAS using WIF or private key string.
    /// Matches C# ClaimGasAsync(string)
    pub async fn claim_gas_from_key(
        &self,
        key: &str,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        self.claim_gas_from_key_with_assert(key, true).await
    }

    /// Claim GAS using WIF or private key string with optional assert emission.
    pub async fn claim_gas_from_key_with_assert(
        &self,
        key: &str,
        add_assert: bool,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        let key_pair = RpcUtility::get_key_pair(key).map_err(std::io::Error::other)?;
        self.claim_gas_with_assert(&key_pair, add_assert).await
    }

    /// Claim GAS with optional assert emission.
    pub async fn claim_gas_with_assert(
        &self,
        key: &KeyPair,
        add_assert: bool,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        let sender_script = Contract::create_signature_redeem_script(key.get_public_key_point()?);
        let sender = UInt160::from_script(&sender_script);

        self.claim_gas_from_account_with_assert(&sender, key, add_assert)
            .await
    }

    /// Claim GAS from specific account
    /// Matches C# `ClaimGasAsync` with account parameter
    pub async fn claim_gas_from_account(
        &self,
        account: &UInt160,
        key: &KeyPair,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        self.claim_gas_from_account_with_assert(account, key, true)
            .await
    }

    /// Claim GAS from specific account with optional assert emission.
    pub async fn claim_gas_from_account_with_assert(
        &self,
        account: &UInt160,
        key: &KeyPair,
        add_assert: bool,
    ) -> Result<Transaction, Box<dyn std::error::Error>> {
        let neo_balance = self.nep17_api.balance_of(&neo_hash(), account).await?;

        if neo_balance == BigInt::from(0) {
            return Err("No NEO balance to claim GAS from".into());
        }

        // Transfer NEO to self to trigger GAS claim
        let tx = self
            .nep17_api
            .create_transfer_tx_with_from_and_assert(
                &neo_hash(),
                account,
                key,
                account,
                neo_balance,
                None,
                add_assert,
            )
            .await?;

        let _hash = self.rpc_client.send_raw_transaction(&tx).await?;
        Ok(tx)
    }

    /// Transfer NEP17 token
    /// Matches C# `TransferAsync`
    pub async fn transfer(
        &self,
        token_hash: &str,
        key: &KeyPair,
        to_address: &str,
        amount: BigInt,
        data: Option<serde_json::Value>,
    ) -> Result<(Transaction, String), Box<dyn std::error::Error>> {
        self.transfer_with_assert(token_hash, key, to_address, amount, data, true)
            .await
    }

    /// Transfer NEP17 token with optional assert emission.
    pub async fn transfer_with_assert(
        &self,
        token_hash: &str,
        key: &KeyPair,
        to_address: &str,
        amount: BigInt,
        data: Option<serde_json::Value>,
        add_assert: bool,
    ) -> Result<(Transaction, String), Box<dyn std::error::Error>> {
        let token_script_hash =
            RpcUtility::get_script_hash(token_hash, &self.rpc_client.protocol_settings)?;
        let to = RpcUtility::get_script_hash(to_address, &self.rpc_client.protocol_settings)?;

        let tx = self
            .nep17_api
            .create_transfer_tx_with_assert(&token_script_hash, key, &to, amount, data, add_assert)
            .await?;

        let hash = self.rpc_client.send_raw_transaction(&tx).await?;
        Ok((tx, hash.to_string()))
    }

    /// Transfer NEP17 token using decimal amount and WIF/private key string.
    /// Matches C# TransferAsync(string tokenHash, string fromKey, string toAddress, decimal amount).
    pub async fn transfer_decimal_from_key(
        &self,
        token_hash: &str,
        from_key: &str,
        to_address: &str,
        amount: BigDecimal,
        data: Option<serde_json::Value>,
    ) -> Result<(Transaction, String), Box<dyn std::error::Error>> {
        self.transfer_decimal_from_key_with_assert(
            token_hash, from_key, to_address, amount, data, true,
        )
        .await
    }

    /// Transfer NEP17 token using decimal amount and WIF/private key string with optional assert.
    pub async fn transfer_decimal_from_key_with_assert(
        &self,
        token_hash: &str,
        from_key: &str,
        to_address: &str,
        amount: BigDecimal,
        data: Option<serde_json::Value>,
        add_assert: bool,
    ) -> Result<(Transaction, String), Box<dyn std::error::Error>> {
        let key_pair = RpcUtility::get_key_pair(from_key).map_err(std::io::Error::other)?;
        self.transfer_decimal_with_assert(
            token_hash, &key_pair, to_address, amount, data, add_assert,
        )
        .await
    }

    /// Transfer NEP17 token using decimal amount.
    /// Matches C# TransferAsync(string tokenHash, string fromKey, string toAddress, decimal amount)
    /// after key parsing.
    pub async fn transfer_decimal(
        &self,
        token_hash: &str,
        key: &KeyPair,
        to_address: &str,
        amount: BigDecimal,
        data: Option<serde_json::Value>,
    ) -> Result<(Transaction, String), Box<dyn std::error::Error>> {
        self.transfer_decimal_with_assert(token_hash, key, to_address, amount, data, true)
            .await
    }

    /// Transfer NEP17 token using decimal amount with optional assert emission.
    pub async fn transfer_decimal_with_assert(
        &self,
        token_hash: &str,
        key: &KeyPair,
        to_address: &str,
        amount: BigDecimal,
        data: Option<serde_json::Value>,
        add_assert: bool,
    ) -> Result<(Transaction, String), Box<dyn std::error::Error>> {
        let token_script_hash =
            RpcUtility::get_script_hash(token_hash, &self.rpc_client.protocol_settings)?;
        let decimals = self.nep17_api.decimals(&token_script_hash).await?;
        let amount_integer = amount
            .to_big_integer(decimals)
            .map_err(|err| std::io::Error::other(err.to_string()))?;

        let to = RpcUtility::get_script_hash(to_address, &self.rpc_client.protocol_settings)?;
        let tx = self
            .nep17_api
            .create_transfer_tx_with_assert(
                &token_script_hash,
                key,
                &to,
                amount_integer,
                data,
                add_assert,
            )
            .await?;

        let hash = self.rpc_client.send_raw_transaction(&tx).await?;
        Ok((tx, hash.to_string()))
    }

    #[allow(clippy::too_many_arguments)]
    /// Transfer NEP17 token from multi-sig account.
    /// Matches C# `TransferAsync` multi-sig overload.
    pub async fn transfer_multi_sig(
        &self,
        token_hash: &str,
        m: usize,
        public_keys: Vec<ECPoint>,
        keys: Vec<KeyPair>,
        to: &UInt160,
        amount: BigInt,
        data: Option<serde_json::Value>,
        add_assert: bool,
    ) -> Result<(Transaction, String), Box<dyn std::error::Error>> {
        let token_script_hash =
            RpcUtility::get_script_hash(token_hash, &self.rpc_client.protocol_settings)?;

        let tx = self
            .nep17_api
            .create_transfer_tx_multi_sig_with_assert(
                &token_script_hash,
                m,
                public_keys,
                keys,
                to,
                amount,
                data,
                add_assert,
            )
            .await?;

        let hash = self.rpc_client.send_raw_transaction(&tx).await?;
        Ok((tx, hash.to_string()))
    }

    /// Wait for a transaction to be confirmed.
    /// Matches C# `WaitTransactionAsync`
    pub async fn wait_transaction(
        &self,
        tx: &Transaction,
    ) -> Result<RpcTransaction, Box<dyn std::error::Error>> {
        self.wait_transaction_with_timeout(tx, 60).await
    }

    /// Wait for a transaction to be confirmed with timeout in seconds.
    pub async fn wait_transaction_with_timeout(
        &self,
        tx: &Transaction,
        timeout_seconds: u64,
    ) -> Result<RpcTransaction, Box<dyn std::error::Error>> {
        // Wait for transaction to be included in a block
        let tx_hash = tx.hash();
        let timeout = std::time::Duration::from_secs(timeout_seconds);
        let poll_interval = std::cmp::max(1, self.rpc_client.protocol_settings.ms_per_block / 2);
        let poll_duration = tokio::time::Duration::from_millis(poll_interval);
        let deadline = std::time::Instant::now() + timeout;

        while std::time::Instant::now() < deadline {
            // Check if transaction is in a block
            if let Ok(rpc_tx) = self.rpc_client.get_transaction(&tx_hash.to_string()).await {
                if rpc_tx.confirmations.is_some() {
                    return Ok(rpc_tx);
                }
            } else {
                // Transaction not found yet, continue waiting
            }

            tokio::time::sleep(poll_duration).await;
        }

        Err(Box::new(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            "Timeout while waiting for transaction confirmation",
        )))
    }

    /// Get account state including balances
    /// Matches C# `GetAccountStateAsync`
    pub async fn get_account_state(
        &self,
        account: &str,
    ) -> Result<WalletAccountState, Box<dyn std::error::Error>> {
        let account_hash =
            RpcUtility::get_script_hash(account, &self.rpc_client.protocol_settings)?;

        // Get NEO and GAS balances
        let neo_balance = self
            .nep17_api
            .balance_of(&neo_hash(), &account_hash)
            .await?;

        let gas_balance = self
            .nep17_api
            .balance_of(&gas_hash(), &account_hash)
            .await?;

        let unclaimed_gas = self.get_unclaimed_gas_from_hash(&account_hash).await?;

        Ok(WalletAccountState {
            address: WalletHelper::to_address(
                &account_hash,
                self.rpc_client.protocol_settings.address_version,
            ),
            neo_balance: neo_balance.to_u32().unwrap_or(0),
            gas_balance: gas_balance.to_f64().unwrap_or(0.0) / gas_factor() as f64,
            unclaimed_gas,
        })
    }
}

fn neo_hash() -> UInt160 {
    NeoToken::new().hash()
}

fn gas_hash() -> UInt160 {
    GasToken::new().hash()
}

fn gas_factor() -> u64 {
    10u64.saturating_pow(u32::from(GasToken::new().decimals()))
}

/// Lightweight account snapshot returned by wallet RPC helpers
#[derive(Debug, Clone)]
pub struct WalletAccountState {
    /// Account address
    pub address: String,
    /// NEO balance
    pub neo_balance: u32,
    /// GAS balance
    pub gas_balance: f64,
    /// Unclaimed GAS
    pub unclaimed_gas: f64,
}

#[cfg(test)]
mod tests {
    use super::*;
    use base64::{engine::general_purpose, Engine as _};
    use mockito::{Matcher, Server};
    use neo_config::ProtocolSettings;
    use neo_json::{JObject, JToken};
    use neo_primitives::UInt256;
    use neo_vm::op_code::OpCode;
    use neo_vm::ScriptBuilder;
    use regex::escape;
    use reqwest::Url;
    use std::fs;
    use std::net::TcpListener;
    use std::path::PathBuf;
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

    fn load_rpc_case_result(name: &str) -> JObject {
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
            if case_name.eq_ignore_ascii_case(name) {
                let response = obj
                    .get("Response")
                    .and_then(|value| value.as_object())
                    .expect("case response");
                let result = response
                    .get("result")
                    .and_then(|value| value.as_object())
                    .expect("case result");
                return result.clone();
            }
        }
        panic!("RpcTestCases.json missing case: {name}");
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

        sb.emit_push_int(neo_core::smart_contract::call_flags::CallFlags::ALL.bits() as i64);
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
        sb.emit_push_int(neo_core::smart_contract::call_flags::CallFlags::ALL.bits() as i64);
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
        let unclaimed_script =
            build_dynamic_call_script(&neo_hash(), "unclaimedGas", &unclaimed_args);

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
        assert_ne!(tx.script().last().copied(), Some(OpCode::ASSERT as u8));
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
        assert_ne!(tx.script().last().copied(), Some(OpCode::ASSERT as u8));
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
        assert_eq!(tx.script().last().copied(), Some(OpCode::ASSERT as u8));
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
        assert_eq!(tx.script().last().copied(), Some(OpCode::ASSERT as u8));
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
        assert_eq!(tx.script().last().copied(), Some(OpCode::ASSERT as u8));
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
        assert_eq!(tx.script().last().copied(), Some(OpCode::ASSERT as u8));
    }

    #[tokio::test]
    async fn wallet_api_wait_transaction_returns_confirmed_tx() {
        if !localhost_binding_permitted() {
            return;
        }

        let mut settings = ProtocolSettings::default_settings();
        settings.ms_per_block = 2;
        let tx = Transaction::new();

        let result_json = load_rpc_case_result("getrawtransactionasync");
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
        settings.ms_per_block = 2;
        let tx = Transaction::new();

        let mut unconfirmed = load_rpc_case_result("getrawtransactionasync");
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
        let io_err = err.downcast_ref::<std::io::Error>().expect("io error");
        assert_eq!(io_err.kind(), std::io::ErrorKind::TimedOut);
    }
}
