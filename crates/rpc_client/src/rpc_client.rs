// Copyright (C) 2015-2025 The Neo Project.
//
// rpc_client.rs file belongs to the neo project and is free
// software distributed under the MIT software license, see the
// accompanying file LICENSE in the main directory of the
// repository or http://www.opensource.org/licenses/mit-license.php
// for more details.
//
// Redistribution and use in source and binary forms with or without
// modifications are permitted.

use crate::models::{
    RpcContractState, RpcInvokeResult, RpcNep17Balances, RpcNep17Transfers, RpcPlugin, RpcRequest,
    RpcResponse, RpcTransaction,
};
use crate::rpc_exception::RpcException;
use base64::{engine::general_purpose, Engine as _};
use neo_core::neo_io::SerializableExt;
use neo_core::{ProtocolSettings, Signer, Transaction};
use neo_json::{JArray, JObject, JToken};
use regex::Regex;
use reqwest::{Client, Url};
use std::sync::{Arc, OnceLock};
use std::time::{Duration, Instant};

static RPC_NAME_REGEX: OnceLock<Regex> = OnceLock::new();
const MAX_JSON_NESTING: usize = 128;
const DEFAULT_HTTP_TIMEOUT: Duration = Duration::from_secs(30);

/// Outcome and timing for a single RPC call.
#[derive(Debug, Clone)]
pub struct RpcRequestOutcome {
    pub method: String,
    pub elapsed: Duration,
    pub success: bool,
    pub timeout: Duration,
    pub error_code: Option<i32>,
}

/// Hooks that can be used to observe RPC requests for logging/metrics.
type RpcObserverFn = dyn Fn(&RpcRequestOutcome) + Send + Sync;

#[derive(Clone, Default)]
pub struct RpcClientHooks {
    observer: Option<Arc<RpcObserverFn>>,
}

impl RpcClientHooks {
    /// Returns a hook collection without observers (falls back to tracing debug logs).
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an observer called after each RPC request completes.
    pub fn with_observer<F>(mut self, observer: F) -> Self
    where
        F: Fn(&RpcRequestOutcome) + Send + Sync + 'static,
    {
        self.observer = Some(Arc::new(observer));
        self
    }

    fn notify(&self, outcome: RpcRequestOutcome) {
        if let Some(observer) = &self.observer {
            observer(&outcome);
        } else {
            tracing::debug!(
                method = %outcome.method,
                elapsed_ms = outcome.elapsed.as_millis() as u64,
                success = outcome.success,
                timeout_ms = outcome.timeout.as_millis() as u64,
                error_code = outcome.error_code,
                "rpc request finished"
            );
        }
    }
}

/// Builder for configuring an [`RpcClient`] with timeouts and hooks.
pub struct RpcClientBuilder {
    base_address: Url,
    rpc_user: Option<String>,
    rpc_pass: Option<String>,
    protocol_settings: Option<ProtocolSettings>,
    timeout: Duration,
    hooks: RpcClientHooks,
}

impl RpcClientBuilder {
    pub fn new(base_address: Url) -> Self {
        Self {
            base_address,
            rpc_user: None,
            rpc_pass: None,
            protocol_settings: None,
            timeout: DEFAULT_HTTP_TIMEOUT,
            hooks: RpcClientHooks::default(),
        }
    }

    /// Applies basic-auth credentials.
    pub fn with_basic_auth(mut self, user: impl Into<String>, pass: impl Into<String>) -> Self {
        self.rpc_user = Some(user.into());
        self.rpc_pass = Some(pass.into());
        self
    }

    /// Applies optional basic-auth credentials (helper for matching legacy constructor).
    pub fn with_optional_auth(mut self, user: Option<String>, pass: Option<String>) -> Self {
        self.rpc_user = user;
        self.rpc_pass = pass;
        self
    }

    /// Overrides the protocol settings used for serialisation.
    pub fn protocol_settings(mut self, settings: ProtocolSettings) -> Self {
        self.protocol_settings = Some(settings);
        self
    }

    /// Configures the HTTP client timeout.
    pub fn timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Registers hooks for logging/metrics.
    pub fn hooks(mut self, hooks: RpcClientHooks) -> Self {
        self.hooks = hooks;
        self
    }

    pub fn build(self) -> Result<RpcClient, Box<dyn std::error::Error>> {
        let mut client_builder = Client::builder().timeout(self.timeout);

        if let (Some(user), Some(pass)) = (self.rpc_user, self.rpc_pass) {
            let auth = format!("{user}:{pass}");
            let encoded = general_purpose::STANDARD.encode(auth.as_bytes());
            client_builder = client_builder.default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    reqwest::header::AUTHORIZATION,
                    format!("Basic {}", encoded).parse()?,
                );
                headers
            });
        }

        let http_client = client_builder.build()?;

        Ok(RpcClient {
            base_address: self.base_address,
            http_client,
            protocol_settings: Arc::new(self.protocol_settings.unwrap_or_default()),
            request_timeout: self.timeout,
            hooks: self.hooks,
        })
    }
}

/// The RPC client to call NEO RPC methods
/// Matches C# RpcClient
#[derive(Clone)]
pub struct RpcClient {
    base_address: Url,
    http_client: Client,
    pub(crate) protocol_settings: Arc<ProtocolSettings>,
    request_timeout: Duration,
    hooks: RpcClientHooks,
}

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
            DEFAULT_HTTP_TIMEOUT,
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
    fn as_rpc_response(content: &str, throw_on_error: bool) -> Result<RpcResponse, RpcException> {
        let json = JToken::parse(content, MAX_JSON_NESTING)
            .map_err(|e| RpcException::new(-32700, format!("Parse error: {}", e)))?;

        let response_obj = match json {
            JToken::Object(obj) => obj,
            _ => {
                return Err(RpcException::new(
                    -32700,
                    "Invalid response format".to_string(),
                ))
            }
        };

        let mut response = RpcResponse::from_json(&response_obj)
            .map_err(|e| RpcException::new(-32700, format!("Invalid response: {}", e)))?;

        response.raw_response = Some(content.to_string());

        if let Some(ref error) = response.error {
            if throw_on_error {
                return Err(RpcException::new(error.code, error.message.clone()));
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
    ) -> Result<RpcResponse, RpcException> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| RpcException::new(-32603, "No async runtime available".to_string()))?;

        runtime.block_on(self.send_async(request, throw_on_error))
    }

    /// Sends an asynchronous RPC request
    /// Matches C# SendAsync
    pub async fn send_async(
        &self,
        request: RpcRequest,
        throw_on_error: bool,
    ) -> Result<RpcResponse, RpcException> {
        let method = request.method.clone();
        let start = Instant::now();

        let result: Result<RpcResponse, RpcException> = async {
            let request_json = request.to_json().to_string();

            let response = self
                .http_client
                .post(self.base_address.clone())
                .header("Content-Type", "application/json")
                .body(request_json)
                .send()
                .await
                .map_err(|e| RpcException::new(-32603, format!("HTTP error: {}", e)))?;

            let content = response.text().await.map_err(|e| {
                RpcException::new(-32603, format!("Failed to read response: {}", e))
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
                error_code: Some(err.code),
            }),
        }

        result
    }

    /// Sends an RPC request and returns the result
    /// Matches C# RpcSend
    pub fn rpc_send(&self, method: &str, params: Vec<JToken>) -> Result<JToken, RpcException> {
        let request = Self::as_rpc_request(method, params);
        let response = self.send(request, true)?;
        response
            .result
            .ok_or_else(|| RpcException::new(-32603, "No result returned".to_string()))
    }

    /// Sends an async RPC request and returns the result
    /// Matches C# RpcSendAsync
    pub async fn rpc_send_async(
        &self,
        method: &str,
        params: Vec<JToken>,
    ) -> Result<JToken, RpcException> {
        let request = Self::as_rpc_request(method, params);
        let response = self.send_async(request, true).await?;
        response
            .result
            .ok_or_else(|| RpcException::new(-32603, "No result returned".to_string()))
    }

    /// Gets the RPC method name from a function name
    /// Matches C# GetRpcName
    pub fn get_rpc_name(method_name: &str) -> String {
        let regex = RPC_NAME_REGEX.get_or_init(|| Regex::new(r"(.*?)(Hex|Both)?(Async)?").unwrap());

        regex.replace(method_name, "$1").to_lowercase()
    }

    /// Returns the list of plugins reported by the node (matches `getplugins`).
    pub async fn get_plugins(&self) -> Result<Vec<RpcPlugin>, RpcException> {
        let result = self.rpc_send_async("getplugins", vec![]).await?;
        parse_plugins(&result)
    }

    // Blockchain methods

    /// Returns the hash of the tallest block in the main chain
    /// Matches C# GetBestBlockHashAsync
    pub async fn get_best_block_hash(&self) -> Result<String, RpcException> {
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
    ) -> Result<JToken, RpcException> {
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
    pub async fn invoke_script(&self, script: &[u8]) -> Result<RpcInvokeResult, RpcException> {
        self.invoke_script_with_signers(script, &[]).await
    }

    /// Invokes a VM script with optional signer context.
    pub async fn invoke_script_with_signers(
        &self,
        script: &[u8],
        signers: &[Signer],
    ) -> Result<RpcInvokeResult, RpcException> {
        let mut parameters = Vec::with_capacity(2);
        parameters.push(JToken::String(general_purpose::STANDARD.encode(script)));

        if !signers.is_empty() {
            let mut signer_tokens = Vec::with_capacity(signers.len());
            for signer in signers {
                let serialized = serde_json::to_string(signer).map_err(|err| {
                    RpcException::new(
                        -32603,
                        format!("Failed to serialize signer for invokescript: {err}"),
                    )
                })?;
                let token = JToken::parse(&serialized, MAX_JSON_NESTING).map_err(|err| {
                    RpcException::new(
                        -32603,
                        format!("Failed to convert signer JSON to neo-json token: {err}"),
                    )
                })?;
                signer_tokens.push(token);
            }
            parameters.push(JToken::Array(JArray::from(signer_tokens)));
        }

        let result = self.rpc_send_async("invokescript", parameters).await?;

        let obj = token_as_object(result, "invokescript")?;

        RpcInvokeResult::from_json(&obj).map_err(|err| RpcException::new(-32603, err))
    }

    /// Returns the serialized block as hex string
    /// Matches C# GetBlockHexAsync
    pub async fn get_block_hex(&self, hash_or_index: &str) -> Result<String, RpcException> {
        let result = self
            .rpc_send_by_hash_or_index("getblock", hash_or_index, vec![])
            .await?;
        token_as_string(result, "getblock hex")
    }

    /// Returns the block information
    /// Matches C# GetBlockAsync
    pub async fn get_block(
        &self,
        hash_or_index: &str,
    ) -> Result<crate::models::RpcBlock, RpcException> {
        let result = self
            .rpc_send_by_hash_or_index("getblock", hash_or_index, vec![JToken::Boolean(true)])
            .await?;
        let obj = token_as_object(result, "getblock detailed")?;
        crate::models::RpcBlock::from_json(&obj, &self.protocol_settings)
            .map_err(|e| RpcException::new(-32603, format!("Failed to parse block: {}", e)))
    }

    /// Gets the number of block headers in the main chain
    /// Matches C# GetBlockHeaderCountAsync
    pub async fn get_block_header_count(&self) -> Result<u32, RpcException> {
        let result = self.rpc_send_async("getblockheadercount", vec![]).await?;
        token_as_number(result, "getblockheadercount").map(|n| n as u32)
    }

    /// Gets the number of blocks in the main chain
    /// Matches C# GetBlockCountAsync
    pub async fn get_block_count(&self) -> Result<u32, RpcException> {
        let result = self.rpc_send_async("getblockcount", vec![]).await?;
        token_as_number(result, "getblockcount").map(|n| n as u32)
    }

    /// Gets the hash value of the corresponding block based on the specified index
    /// Matches C# GetBlockHashAsync
    pub async fn get_block_hash(&self, index: u32) -> Result<String, RpcException> {
        let result = self
            .rpc_send_async("getblockhash", vec![JToken::Number(index as f64)])
            .await?;
        token_as_string(result, "getblockhash")
    }

    /// Returns the corresponding block header information by hash or index
    /// Matches C# GetBlockHeaderHexAsync
    pub async fn get_block_header_hex(&self, hash_or_index: &str) -> Result<String, RpcException> {
        let result = self
            .rpc_send_by_hash_or_index("getblockheader", hash_or_index, vec![])
            .await?;
        token_as_string(result, "getblockheader hash")
    }

    /// Returns the corresponding block header information by hash or index
    /// Matches C# GetBlockHeaderAsync
    pub async fn get_block_header(
        &self,
        hash_or_index: &str,
    ) -> Result<crate::models::RpcBlockHeader, RpcException> {
        let result = self
            .rpc_send_by_hash_or_index("getblockheader", hash_or_index, vec![JToken::Boolean(true)])
            .await?;
        let obj = token_as_object(result, "getblockheader")?;
        crate::models::RpcBlockHeader::from_json(&obj, &self.protocol_settings)
            .map_err(|e| RpcException::new(-32603, format!("Failed to parse block header: {}", e)))
    }

    /// Fetches contract state.
    pub async fn get_contract_state(&self, hash: &str) -> Result<RpcContractState, RpcException> {
        let result = self
            .rpc_send_async("getcontractstate", vec![JToken::String(hash.to_string())])
            .await?;
        let obj = token_as_object(result, "getcontractstate")?;
        RpcContractState::from_json(&obj).map_err(|err| RpcException::new(-32603, err))
    }

    /// Gets NEP-17 transfers for an address.
    pub async fn get_nep17_transfers(
        &self,
        address: &str,
        start_time: Option<u64>,
        end_time: Option<u64>,
    ) -> Result<RpcNep17Transfers, RpcException> {
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
            .map_err(|err| RpcException::new(-32603, err))
    }

    /// Gets NEP-17 balances for an address.
    pub async fn get_nep17_balances(
        &self,
        address: &str,
    ) -> Result<RpcNep17Balances, RpcException> {
        let result = self
            .rpc_send_async(
                "getnep17balances",
                vec![JToken::String(address.to_string())],
            )
            .await?;
        let obj = token_as_object(result, "getnep17balances")?;
        RpcNep17Balances::from_json(&obj, &self.protocol_settings)
            .map_err(|err| RpcException::new(-32603, err))
    }

    /// Retrieves a transaction by hash.
    pub async fn get_transaction(&self, hash: &str) -> Result<RpcTransaction, RpcException> {
        let result = self
            .rpc_send_async(
                "getrawtransaction",
                vec![JToken::String(hash.to_string()), JToken::Boolean(true)],
            )
            .await?;
        let obj = token_as_object(result, "getrawtransaction")?;
        RpcTransaction::from_json(&obj, &self.protocol_settings)
            .map_err(|err| RpcException::new(-32603, err))
    }

    /// Broadcasts a raw transaction.
    pub async fn send_raw_transaction(&self, _tx: &Transaction) -> Result<bool, RpcException> {
        let raw = _tx
            .to_array()
            .map_err(|err| RpcException::new(-32603, format!("serialization failed: {err}")))?;
        let hex = hex::encode(raw);
        let result = self
            .rpc_send_async("sendrawtransaction", vec![JToken::String(hex)])
            .await?;
        token_as_boolean(result, "sendrawtransaction")
    }
}

fn token_as_string(token: JToken, context: &str) -> Result<String, RpcException> {
    match token {
        JToken::String(value) => Ok(value),
        _ => Err(RpcException::new(
            -32603,
            format!("{context}: expected string token"),
        )),
    }
}

fn token_as_number(token: JToken, context: &str) -> Result<f64, RpcException> {
    match token {
        JToken::Number(value) => Ok(value),
        _ => Err(RpcException::new(
            -32603,
            format!("{context}: expected numeric token"),
        )),
    }
}

fn token_as_object(token: JToken, context: &str) -> Result<JObject, RpcException> {
    match token {
        JToken::Object(obj) => Ok(obj),
        _ => Err(RpcException::new(
            -32603,
            format!("{context}: expected object token"),
        )),
    }
}

fn token_as_boolean(token: JToken, context: &str) -> Result<bool, RpcException> {
    match token {
        JToken::Boolean(value) => Ok(value),
        _ => Err(RpcException::new(
            -32603,
            format!("{context}: expected boolean token"),
        )),
    }
}

fn parse_plugins(result: &JToken) -> Result<Vec<RpcPlugin>, RpcException> {
    let array = result
        .as_array()
        .ok_or_else(|| RpcException::new(-32603, "getplugins returned non-array".into()))?;

    array
        .iter()
        .map(|item| {
            let token = item
                .as_ref()
                .ok_or_else(|| RpcException::new(-32603, "plugin entry was null".into()))?;
            let obj = token.as_object().ok_or_else(|| {
                RpcException::new(-32603, "plugin entry was not an object".into())
            })?;
            RpcPlugin::from_json(obj)
                .map_err(|err| RpcException::new(-32603, format!("invalid plugin entry: {err}")))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hyper::service::{make_service_fn, service_fn};
    use hyper::{Body, Response, Server as HyperServer};
    use mockito::Server;
    use std::convert::Infallible;
    use std::net::{SocketAddr, TcpListener};
    use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
    use std::sync::Arc;
    use tokio::sync::oneshot;

    async fn start_slow_server(
        delay: Duration,
        body: &'static str,
    ) -> (SocketAddr, oneshot::Sender<()>) {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind listener");
        let addr = listener.local_addr().expect("local addr");
        listener
            .set_nonblocking(true)
            .expect("configure test listener");
        let (shutdown_tx, shutdown_rx) = oneshot::channel::<()>();

        let server = HyperServer::from_tcp(listener)
            .expect("server from tcp")
            .serve(make_service_fn(move |_| {
                let body = body.to_string();
                async move {
                    Ok::<_, Infallible>(service_fn(move |_| {
                        let body = body.clone();
                        async move {
                            tokio::time::sleep(delay).await;
                            Ok::<_, Infallible>(Response::new(Body::from(body.clone())))
                        }
                    }))
                }
            }));

        let graceful = server.with_graceful_shutdown(async {
            let _ = shutdown_rx.await;
        });
        tokio::spawn(graceful);

        (addr, shutdown_tx)
    }

    #[tokio::test]
    async fn request_hooks_fire_on_successful_call() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(r#"{"jsonrpc":"2.0","id":1,"result":7}"#)
            .create();

        let calls = Arc::new(AtomicUsize::new(0));
        let successful = Arc::new(AtomicBool::new(false));
        let hooks = RpcClientHooks::new().with_observer({
            let calls = Arc::clone(&calls);
            let successful = Arc::clone(&successful);
            move |outcome: &RpcRequestOutcome| {
                calls.fetch_add(1, Ordering::SeqCst);
                if outcome.success {
                    successful.store(true, Ordering::SeqCst);
                }
                assert_eq!(outcome.method, "getblockcount");
                assert_eq!(outcome.error_code, None);
            }
        });

        let url = Url::parse(&server.url()).unwrap();
        let client = RpcClient::builder(url).hooks(hooks).build().unwrap();

        let result = client
            .rpc_send_async("getblockcount", vec![])
            .await
            .expect("rpc call succeeds");
        assert_eq!(result.as_number().unwrap() as u32, 7);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert!(successful.load(Ordering::SeqCst));
    }

    #[tokio::test]
    async fn rpc_client_respects_timeout_and_notifies_hooks() {
        let (addr, shutdown_tx) = start_slow_server(
            Duration::from_millis(200),
            r#"{"jsonrpc":"2.0","id":1,"result":1}"#,
        )
        .await;
        let url = Url::parse(&format!("http://{}", addr)).unwrap();

        let notified = Arc::new(AtomicBool::new(false));
        let hooks = RpcClientHooks::new().with_observer({
            let notified = Arc::clone(&notified);
            move |outcome: &RpcRequestOutcome| {
                notified.store(true, Ordering::SeqCst);
                assert_eq!(outcome.method, "slowcall");
                assert!(!outcome.success);
                assert!(outcome.elapsed >= Duration::from_millis(50));
                assert!(outcome.error_code.is_some());
            }
        });

        let client = RpcClient::builder(url)
            .timeout(Duration::from_millis(50))
            .hooks(hooks)
            .build()
            .unwrap();

        let result = client.rpc_send_async("slowcall", vec![]).await;
        assert!(result.is_err());
        assert!(notified.load(Ordering::SeqCst));
        let _ = shutdown_tx.send(());
    }

    #[test]
    fn parse_plugins_supports_category() {
        let mut plugin_obj = JObject::new();
        plugin_obj.insert("name".to_string(), JToken::String("RpcServer".into()));
        plugin_obj.insert("version".to_string(), JToken::String("1.2.3".into()));
        plugin_obj.insert("category".to_string(), JToken::String("Rpc".into()));
        plugin_obj.insert(
            "interfaces".to_string(),
            JToken::Array(JArray::from(vec![JToken::String("IBlock".into())])),
        );

        let array = JToken::Array(JArray::from(vec![JToken::Object(plugin_obj)]));
        let parsed = parse_plugins(&array).expect("parse plugins");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "RpcServer");
        assert_eq!(parsed[0].version, "1.2.3");
        assert_eq!(parsed[0].category.as_deref(), Some("Rpc"));
        assert_eq!(parsed[0].interfaces, vec!["IBlock".to_string()]);
    }

    #[test]
    fn parse_plugins_errors_on_non_object() {
        let array = JToken::Array(JArray::from(vec![JToken::String("bad".into())]));
        let err = parse_plugins(&array).expect_err("should fail");
        assert_eq!(err.code, -32603);
    }

    #[tokio::test]
    async fn get_plugins_parses_category_over_rpc() {
        let mut server = Server::new_async().await;
        let _m = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(
                r#"{
                    "jsonrpc":"2.0",
                    "id":1,
                    "result":[
                        {"name":"RpcServer","version":"1.2.3","category":"Rpc","interfaces":[]}
                    ]
                }"#,
            )
            .create();

        let url = Url::parse(&server.url()).unwrap();
        let client = RpcClient::builder(url).build().unwrap();
        let plugins = client.get_plugins().await.expect("plugins parsed");
        assert_eq!(plugins.len(), 1);
        assert_eq!(plugins[0].name, "RpcServer");
        assert_eq!(plugins[0].version, "1.2.3");
        assert_eq!(plugins[0].category.as_deref(), Some("Rpc"));
    }

    #[tokio::test]
    async fn get_nep17_balances_parses_rpc_payload() {
        let mut server = Server::new_async().await;
        let address = neo_core::UInt160::zero().to_address();
        let body = format!(
            r#"{{"jsonrpc":"2.0","id":1,"result":{{"balance":[{{"assethash":"0x0000000000000000000000000000000000000000","amount":"5","lastupdatedblock":7}}],"address":"{address}"}}}}"#
        );

        let _m = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let url = Url::parse(&server.url()).unwrap();
        let client = RpcClient::builder(url).build().unwrap();
        let balances = client
            .get_nep17_balances(&address)
            .await
            .expect("parse balances");
        assert_eq!(balances.user_script_hash, neo_core::UInt160::zero());
        assert_eq!(balances.balances.len(), 1);
        assert_eq!(balances.balances[0].last_updated_block, 7);
    }

    #[tokio::test]
    async fn get_nep17_transfers_parses_rpc_payload() {
        let mut server = Server::new_async().await;
        let address = neo_core::UInt160::zero().to_address();
        let tx_hash = neo_core::UInt256::zero();
        let body = format!(
            r#"{{"jsonrpc":"2.0","id":1,"result":{{"sent":[{{"timestamp":1,"assethash":"0x0000000000000000000000000000000000000000","transferaddress":"{address}","amount":"11","blockindex":2,"transfernotifyindex":0,"txhash":"{tx_hash}"}}],"received":[],"address":"{address}"}}}}"#
        );

        let _m = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let url = Url::parse(&server.url()).unwrap();
        let client = RpcClient::builder(url).build().unwrap();
        let transfers = client
            .get_nep17_transfers(&address, None, None)
            .await
            .expect("parse transfers");
        assert_eq!(transfers.user_script_hash, neo_core::UInt160::zero());
        assert_eq!(transfers.sent.len(), 1);
        assert_eq!(transfers.sent[0].block_index, 2);
        assert_eq!(transfers.sent[0].tx_hash, tx_hash);
    }

    #[tokio::test]
    async fn send_raw_transaction_serializes_and_parses_bool() {
        let mut server = Server::new_async().await;
        let tx = Transaction::new();
        let raw = tx.to_array().expect("serialize tx");
        let hex = hex::encode(raw);

        let _m = server
            .mock("POST", "/")
            .with_status(200)
            .match_body(mockito::Matcher::Regex(format!(
                r#"\"method\":\"sendrawtransaction\".*\"params\":\[\s*\"{hex}\"\s*\]"#
            )))
            .with_header("content-type", "application/json")
            .with_body(r#"{"jsonrpc":"2.0","id":1,"result":true}"#)
            .create();

        let url = Url::parse(&server.url()).unwrap();
        let client = RpcClient::builder(url).build().unwrap();
        let accepted = client
            .send_raw_transaction(&tx)
            .await
            .expect("rpc call should succeed");
        assert!(accepted);
    }

    #[tokio::test]
    async fn get_contract_state_parses_rpc_payload() {
        let mut server = Server::new_async().await;
        let nef = neo_core::smart_contract::NefFile {
            compiler: "neo".into(),
            source: "src".into(),
            tokens: Vec::new(),
            script: vec![1, 2, 3],
            checksum: 42,
        };
        let manifest = neo_core::smart_contract::ContractManifest::new("Contract".into());
        let state = RpcContractState {
            contract_state: neo_core::smart_contract::ContractState {
                id: 1,
                update_counter: 2,
                hash: neo_core::UInt160::zero(),
                nef,
                manifest,
            },
        };
        let result_json = state.to_json().expect("state json").to_string();
        let body = format!(r#"{{"jsonrpc":"2.0","id":1,"result":{result_json}}}"#);

        let _m = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let url = Url::parse(&server.url()).unwrap();
        let client = RpcClient::builder(url).build().unwrap();
        let parsed = client
            .get_contract_state(&neo_core::UInt160::zero().to_string())
            .await
            .expect("parse contract state");

        assert_eq!(parsed.contract_state.id, 1);
        assert_eq!(parsed.contract_state.update_counter, 2);
        assert_eq!(parsed.contract_state.nef.checksum, 42);
    }

    #[tokio::test]
    async fn get_transaction_parses_rpc_payload() {
        let mut server = Server::new_async().await;
        let mut tx = Transaction::new();
        tx.set_script(vec![1, 2, 3, 4]);
        tx.set_valid_until_block(5);
        let tx_json =
            crate::utility::transaction_to_json(&tx, &ProtocolSettings::default_settings())
                .to_string();
        let body = format!(r#"{{"jsonrpc":"2.0","id":1,"result":{tx_json}}}"#);

        let _m = server
            .mock("POST", "/")
            .with_status(200)
            .with_header("content-type", "application/json")
            .with_body(body)
            .create();

        let url = Url::parse(&server.url()).unwrap();
        let client = RpcClient::builder(url).build().unwrap();
        let parsed = client
            .get_transaction("0x00")
            .await
            .expect("parse transaction");

        assert_eq!(parsed.transaction.script(), tx.script());
    }
}
