// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_client/client.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use super::super::models::{
    RpcContractState, RpcInvokeResult, RpcNep11Balances, RpcNep11Transfers, RpcNep17Balances,
    RpcNep17Transfers, RpcPlugin, RpcRequest, RpcResponse, RpcTransaction,
};
use super::super::ClientRpcError;
use base64::{engine::general_purpose, Engine as _};
use neo_config::ProtocolSettings;
use neo_core::{Signer, Transaction};
use neo_io::{BinaryWriter, Serializable};
use neo_json::{JArray, JObject, JToken};
use regex::Regex;
use reqwest::{Client, Url};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::builder::RpcClientBuilder;
use super::helpers::{
    parse_plugins, token_as_boolean, token_as_number, token_as_object, token_as_string,
};
use super::hooks::RpcRequestOutcome;
use super::{RpcClient, RpcClientHooks, MAX_JSON_NESTING, RPC_NAME_REGEX};

impl RpcClient {
    /// Creates a configurable builder for the RPC client.
    pub fn builder(url: Url) -> RpcClientBuilder {
        RpcClientBuilder::new(url)
    }

    /// Creates a new RPC client
    /// Matches C# constructor
    pub fn new(
        url: Url,
        rpc_user: Option<String>,
        rpc_pass: Option<String>,
        protocol_settings: Option<ProtocolSettings>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        RpcClientBuilder::new(url)
            .with_optional_auth(rpc_user, rpc_pass)
            .protocol_settings(protocol_settings.unwrap_or_default())
            .build()
    }

    /// Creates a new RPC client with an existing HTTP client
    /// Matches C# constructor
    pub fn with_client(
        client: Client,
        url: Url,
        protocol_settings: Option<ProtocolSettings>,
    ) -> Self {
        Self::with_client_config(
            client,
            url,
            protocol_settings.unwrap_or_default(),
            RpcClientHooks::default(),
            super::DEFAULT_HTTP_TIMEOUT,
        )
    }

    /// Creates a new RPC client with an existing HTTP client and hook/timeout configuration.
    pub fn with_client_config(
        client: Client,
        url: Url,
        protocol_settings: ProtocolSettings,
        hooks: RpcClientHooks,
        timeout: Duration,
    ) -> Self {
        Self {
            base_address: url,
            http_client: client,
            protocol_settings: Arc::new(protocol_settings),
            request_timeout: timeout,
            hooks,
        }
    }

    /// Creates an RPC request
    /// Matches C# AsRpcRequest
    fn as_rpc_request(method: &str, params: Vec<JToken>) -> RpcRequest {
        RpcRequest {
            id: JToken::Number(1.0),
            json_rpc: "2.0".to_string(),
            method: method.to_string(),
            params,
        }
    }

    /// Processes an RPC response
    /// Matches C# AsRpcResponse
    fn as_rpc_response(content: &str, throw_on_error: bool) -> Result<RpcResponse, ClientRpcError> {
        let json = JToken::parse(content, MAX_JSON_NESTING)
            .map_err(|e| ClientRpcError::new(-32700, format!("Parse error: {}", e)))?;
        let response_obj = match json {
            JToken::Object(obj) => obj,
            _ => {
                return Err(ClientRpcError::new(
                    -32700,
                    "Invalid response format".to_string(),
                ))
            }
        };

        let mut response = RpcResponse::from_json(&response_obj)
            .map_err(|e| ClientRpcError::new(-32700, format!("Invalid response: {}", e)))?;

        response.raw_response = Some(content.to_string());

        if let Some(ref error) = response.error {
            if throw_on_error {
                return Err(ClientRpcError::new(error.code, error.message.clone()));
            }
        }

        Ok(response)
    }

    /// Sends a synchronous RPC request
    /// Matches C# Send
    pub fn send(
        &self,
        request: RpcRequest,
        throw_on_error: bool,
    ) -> Result<RpcResponse, ClientRpcError> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| ClientRpcError::new(-32603, "No async runtime available".to_string()))?;

        runtime.block_on(self.send_async(request, throw_on_error))
    }

    /// Sends an asynchronous RPC request
    /// Matches C# SendAsync
    pub async fn send_async(
        &self,
        request: RpcRequest,
        throw_on_error: bool,
    ) -> Result<RpcResponse, ClientRpcError> {
        let method = request.method.clone();
        let start = Instant::now();

        let result: Result<RpcResponse, ClientRpcError> = async {
            let request_json = request.to_json().to_string();

            let response = self
                .http_client
                .post(self.base_address.clone())
                .header("Content-Type", "application/json")
                .body(request_json)
                .send()
                .await
                .map_err(|e| ClientRpcError::new(-32603, format!("HTTP error: {}", e)))?;

            let content = response.text().await.map_err(|e| {
                ClientRpcError::new(-32603, format!("Failed to read response: {}", e))
            })?;

            Self::as_rpc_response(&content, throw_on_error)
        }
        .await;

        let elapsed = start.elapsed();
        match &result {
            Ok(response) => {
                let error_code = response.error.as_ref().map(|e| e.code);
                self.hooks.notify(RpcRequestOutcome {
                    method,
                    elapsed,
                    success: error_code.is_none(),
                    timeout: self.request_timeout,
                    error_code,
                });
            }
            Err(err) => self.hooks.notify(RpcRequestOutcome {
                method,
                elapsed,
                success: false,
                timeout: self.request_timeout,
                error_code: Some(err.code()),
            }),
        }

        result
    }

    /// Sends an RPC request and returns the result
    /// Matches C# RpcSend
    pub fn rpc_send(&self, method: &str, params: Vec<JToken>) -> Result<JToken, ClientRpcError> {
        let request = Self::as_rpc_request(method, params);
        let response = self.send(request, true)?;
        response
            .result
            .ok_or_else(|| ClientRpcError::new(-32603, "No result returned".to_string()))
    }

    /// Sends an async RPC request and returns the result
    /// Matches C# RpcSendAsync
    pub async fn rpc_send_async(
        &self,
        method: &str,
        params: Vec<JToken>,
    ) -> Result<JToken, ClientRpcError> {
        let request = Self::as_rpc_request(method, params);
        let response = self.send_async(request, true).await?;
        response
            .result
            .ok_or_else(|| ClientRpcError::new(-32603, "No result returned".to_string()))
    }

    /// Gets the RPC method name from a function name
    /// Matches C# GetRpcName
    pub fn get_rpc_name(method_name: &str) -> String {
        let regex = RPC_NAME_REGEX.get_or_init(|| Regex::new(r"(.*?)(Hex|Both)?(Async)?").unwrap());

        regex.replace(method_name, "$1").to_lowercase()
    }

    /// Returns a list of plugins loaded by the node (matches `listplugins`).
    pub async fn get_plugins(&self) -> Result<Vec<RpcPlugin>, ClientRpcError> {
        let result = self.rpc_send_async("listplugins", vec![]).await?;
        parse_plugins(&result)
    }

    // Blockchain methods

    /// Returns the hash of the tallest block in the main chain
    /// Matches C# GetBestBlockHashAsync
    pub async fn get_best_block_hash(&self) -> Result<String, ClientRpcError> {
        let result = self.rpc_send_async("getbestblockhash", vec![]).await?;
        token_as_string(result, "getbestblockhash")
    }

    /// Internal helper for sending requests by hash or index
    /// Matches C# RpcSendByHashOrIndexAsync
    async fn rpc_send_by_hash_or_index(
        &self,
        rpc_name: &str,
        hash_or_index: &str,
        arguments: Vec<JToken>,
    ) -> Result<JToken, ClientRpcError> {
        let mut params = vec![];

        if let Ok(index) = hash_or_index.parse::<u32>() {
            params.push(JToken::Number(index as f64));
        } else {
            params.push(JToken::String(hash_or_index.to_string()));
        }

        params.extend(arguments);
        self.rpc_send_async(rpc_name, params).await
    }

    /// Invokes a VM script without affecting blockchain state.
    pub async fn invoke_script(&self, script: &[u8]) -> Result<RpcInvokeResult, ClientRpcError> {
        self.invoke_script_with_signers(script, &[]).await
    }

    /// Invokes a VM script with optional signer context.
    pub async fn invoke_script_with_signers(
        &self,
        script: &[u8],
        signers: &[Signer],
    ) -> Result<RpcInvokeResult, ClientRpcError> {
        let mut parameters = Vec::with_capacity(2);
        parameters.push(JToken::String(general_purpose::STANDARD.encode(script)));

        if !signers.is_empty() {
            let mut signer_tokens = Vec::with_capacity(signers.len());
            for signer in signers {
                let serialized = serde_json::to_string(signer).map_err(|err| {
                    ClientRpcError::new(
                        -32603,
                        format!("Failed to serialize signer for invokescript: {err}"),
                    )
                })?;
                let token = JToken::parse(&serialized, MAX_JSON_NESTING).map_err(|err| {
                    ClientRpcError::new(
                        -32603,
                        format!("Failed to parse signer for invokescript: {err}"),
                    )
                })?;
                signer_tokens.push(token);
            }
            parameters.push(JToken::Array(JArray::from(signer_tokens)));
        }

        let result = self.rpc_send_async("invokescript", parameters).await?;
        let obj = token_as_object(result, "invokescript")?;
        RpcInvokeResult::from_json(&obj).map_err(|err| ClientRpcError::new(-32603, err))
    }

    /// Invokes a function on a contract.
    pub async fn invoke_function(
        &self,
        contract_hash: &str,
        operation: &str,
        params: &[JToken],
    ) -> Result<RpcInvokeResult, ClientRpcError> {
        let result = self
            .rpc_send_async(
                "invokefunction",
                vec![
                    JToken::String(contract_hash.to_string()),
                    JToken::String(operation.to_string()),
                    JToken::Array(JArray::from(params.to_vec())),
                ],
            )
            .await?;
        let obj = token_as_object(result, "invokefunction")?;
        RpcInvokeResult::from_json(&obj).map_err(|err| ClientRpcError::new(-32603, err))
    }

    /// Gets a block by hash or index (verbose).
    ///
    /// This matches the default behaviour of the C# client where `verbose = true`.
    pub async fn get_block(&self, hash_or_index: &str) -> Result<JToken, ClientRpcError> {
        self.get_block_with_verbosity(hash_or_index, true).await
    }

    /// Gets a block by hash or index with explicit verbosity control.
    ///
    /// - `verbose = true` returns a JSON block object
    /// - `verbose = false` returns a raw hex string
    pub async fn get_block_with_verbosity(
        &self,
        hash_or_index: &str,
        verbose: bool,
    ) -> Result<JToken, ClientRpcError> {
        let verbose_token = JToken::Boolean(verbose);
        self.rpc_send_by_hash_or_index("getblock", hash_or_index, vec![verbose_token])
            .await
    }

    /// Gets a raw block (hex) by hash or index.
    pub async fn get_block_hex(&self, hash_or_index: &str) -> Result<String, ClientRpcError> {
        let result = self.get_block_with_verbosity(hash_or_index, false).await?;
        token_as_string(result, "getblock")
    }

    /// Gets a block count
    /// Matches C# GetBlockCountAsync
    pub async fn get_block_count(&self) -> Result<u32, ClientRpcError> {
        let result = self.rpc_send_async("getblockcount", vec![]).await?;
        token_as_number(result, "getblockcount").map(|n| n as u32)
    }

    /// Gets a block hash by index.
    /// Matches C# GetBlockHashAsync
    pub async fn get_block_hash(&self, index: u32) -> Result<String, ClientRpcError> {
        let result = self
            .rpc_send_async("getblockhash", vec![JToken::Number(index as f64)])
            .await?;
        token_as_string(result, "getblockhash")
    }

    /// Gets a block header count.
    /// Matches C# GetBlockHeaderCountAsync
    pub async fn get_block_header_count(&self) -> Result<u32, ClientRpcError> {
        let result = self.rpc_send_async("getblockheadercount", vec![]).await?;
        token_as_number(result, "getblockheadercount").map(|n| n as u32)
    }

    /// Gets a block by hash or index (verbose)
    pub async fn get_block_verbose(
        &self,
        hash_or_index: &str,
    ) -> Result<super::super::models::RpcBlock, ClientRpcError> {
        let result = self.get_block(hash_or_index).await?;
        let obj = token_as_object(result, "getblock")?;
        super::super::models::RpcBlock::from_json(&obj, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err))
    }

    /// Gets a block header by hash or index (verbose).
    pub async fn get_block_header(&self, hash_or_index: &str) -> Result<JToken, ClientRpcError> {
        self.rpc_send_by_hash_or_index("getblockheader", hash_or_index, vec![JToken::Boolean(true)])
            .await
    }

    /// Gets a raw block header (hex) by hash or index.
    pub async fn get_block_header_hex(
        &self,
        hash_or_index: &str,
    ) -> Result<String, ClientRpcError> {
        let result = self
            .rpc_send_by_hash_or_index(
                "getblockheader",
                hash_or_index,
                vec![JToken::Boolean(false)],
            )
            .await?;
        token_as_string(result, "getblockheader")
    }

    /// Gets a block header by hash or index (verbose)
    pub async fn get_block_header_verbose(
        &self,
        hash_or_index: &str,
    ) -> Result<super::super::models::RpcBlockHeader, ClientRpcError> {
        let result = self.get_block_header(hash_or_index).await?;
        let obj = token_as_object(result, "getblockheader")?;
        super::super::models::RpcBlockHeader::from_json(&obj, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err))
    }

    /// Gets NEP-17 transfers.
    pub async fn get_nep17_transfers(
        &self,
        address: &str,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> Result<RpcNep17Transfers, ClientRpcError> {
        let mut params = vec![JToken::String(address.to_string())];
        if let Some(start) = start_time {
            params.push(JToken::Number(start as f64));
        }
        if let Some(end) = end_time {
            params.push(JToken::Number(end as f64));
        }

        let result = self.rpc_send_async("getnep17transfers", params).await?;
        let obj = token_as_object(result, "getnep17transfers")?;
        RpcNep17Transfers::from_json(&obj, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err))
    }

    /// Gets NEP-17 balances.
    pub async fn get_nep17_balances(
        &self,
        address: &str,
    ) -> Result<RpcNep17Balances, ClientRpcError> {
        let result = self
            .rpc_send_async(
                "getnep17balances",
                vec![JToken::String(address.to_string())],
            )
            .await?;
        let obj = token_as_object(result, "getnep17balances")?;
        RpcNep17Balances::from_json(&obj, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err))
    }

    /// Gets NEP-11 transfers.
    pub async fn get_nep11_transfers(
        &self,
        address: &str,
    ) -> Result<RpcNep11Transfers, ClientRpcError> {
        let result = self
            .rpc_send_async(
                "getnep11transfers",
                vec![JToken::String(address.to_string())],
            )
            .await?;
        let obj = token_as_object(result, "getnep11transfers")?;
        RpcNep11Transfers::from_json(&obj, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err))
    }

    /// Gets NEP-11 balances.
    pub async fn get_nep11_balances(
        &self,
        address: &str,
    ) -> Result<RpcNep11Balances, ClientRpcError> {
        let result = self
            .rpc_send_async(
                "getnep11balances",
                vec![JToken::String(address.to_string())],
            )
            .await?;
        let obj = token_as_object(result, "getnep11balances")?;
        RpcNep11Balances::from_json(&obj, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err))
    }

    /// Gets contract state by hash.
    pub async fn get_contract_state(&self, hash: &str) -> Result<RpcContractState, ClientRpcError> {
        let result = self
            .rpc_send_async("getcontractstate", vec![JToken::String(hash.to_string())])
            .await?;
        let obj = token_as_object(result, "getcontractstate")?;
        RpcContractState::from_json(&obj).map_err(|err| ClientRpcError::new(-32603, err))
    }

    /// Gets NEP-11 properties.
    pub async fn get_nep11_properties(
        &self,
        nep11_contract: &str,
        token_id_hex: &str,
    ) -> Result<JObject, ClientRpcError> {
        let params = vec![
            JToken::String(nep11_contract.to_string()),
            JToken::String(token_id_hex.to_string()),
        ];

        let result = self.rpc_send_async("getnep11properties", params).await?;
        token_as_object(result, "getnep11properties")
    }

    /// Retrieves a transaction by hash.
    pub async fn get_transaction(&self, hash: &str) -> Result<RpcTransaction, ClientRpcError> {
        let result = self
            .rpc_send_async(
                "getrawtransaction",
                vec![JToken::String(hash.to_string()), JToken::Boolean(true)],
            )
            .await?;
        let obj = token_as_object(result, "getrawtransaction")?;
        RpcTransaction::from_json(&obj, &self.protocol_settings)
            .map_err(|err| ClientRpcError::new(-32603, err))
    }

    /// Broadcasts a raw transaction.
    pub async fn send_raw_transaction(&self, _tx: &Transaction) -> Result<bool, ClientRpcError> {
        let mut writer = BinaryWriter::new();
        _tx.serialize(&mut writer)
            .map_err(|err| ClientRpcError::new(-32603, format!("serialization failed: {err}")))?;
        let hex = hex::encode(writer.into_bytes());
        let result = self
            .rpc_send_async("sendrawtransaction", vec![JToken::String(hex)])
            .await?;
        token_as_boolean(result, "sendrawtransaction")
    }
}
