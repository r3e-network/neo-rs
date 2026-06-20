use super::super::ClientRpcError;
use super::super::models::{RpcPlugin, RpcRequest, RpcResponse};
use crate::RpcError;
use neo_config::ProtocolSettings;
use regex::Regex;
use reqwest::{Client, Url};
use std::sync::Arc;
use std::time::{Duration, Instant};

use super::builder::RpcClientBuilder;
use super::helpers::parse_plugins;
use super::hooks::RpcRequestOutcome;
use super::{MAX_JSON_NESTING, RPC_NAME_REGEX, RpcClient, RpcClientHooks};
use neo_serialization::json::JToken;

impl RpcClient {
    /// Creates a configurable builder for the RPC client.
    #[must_use]
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
    ) -> Result<Self, RpcError> {
        RpcClientBuilder::new(url)
            .with_optional_auth(rpc_user, rpc_pass)
            .protocol_settings(protocol_settings.unwrap_or_default())
            .build()
    }

    /// Creates a new RPC client with an existing HTTP client
    /// Matches C# constructor
    #[must_use]
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
    #[must_use]
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
    /// Matches C# `AsRpcRequest`
    fn as_rpc_request(method: &str, params: Vec<JToken>) -> RpcRequest {
        RpcRequest {
            id: JToken::Number(1.0),
            json_rpc: "2.0".to_string(),
            method: method.to_string(),
            params,
        }
    }

    /// Processes an RPC response
    /// Matches C# `AsRpcResponse`
    fn as_rpc_response(content: &str, throw_on_error: bool) -> Result<RpcResponse, ClientRpcError> {
        let json = JToken::parse(content, MAX_JSON_NESTING)
            .map_err(|e| ClientRpcError::new(-32700, format!("Parse error: {e}")))?;
        let response_obj = match json {
            JToken::Object(obj) => obj,
            _ => {
                return Err(ClientRpcError::new(
                    -32700,
                    "Invalid response format".to_string(),
                ));
            }
        };

        let mut response = RpcResponse::from_json(&response_obj)
            .map_err(|e| ClientRpcError::new(-32700, format!("Invalid response: {e}")))?;

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
    /// Matches C# `SendAsync`
    pub async fn send_async(
        &self,
        request: RpcRequest,
        throw_on_error: bool,
    ) -> Result<RpcResponse, ClientRpcError> {
        let method = request.method.clone();
        let start = Instant::now();

        let result: Result<RpcResponse, ClientRpcError> = async {
            // Serialize the outgoing request with plain JSON (serde_json default
            // escaping). RPC *responses* must match C# JavaScriptEncoder.Default
            // byte-for-byte (see neo_serialization::json::escape) because clients byte-compare
            // them; a request only needs to be valid JSON the node can parse.
            // JToken::to_string now applies the C# encoder, which would escape the
            // '+' characters in base64 params (decoded identically by any server) —
            // unnecessary noise on requests, so serialize plainly here.
            let request_json = serde_json::to_string(&JToken::Object(request.to_json()))
                .unwrap_or_else(|_| request.to_json().to_string());

            let response = self
                .http_client
                .post(self.base_address.clone())
                .header("Content-Type", "application/json")
                .body(request_json)
                .send()
                .await
                .map_err(|e| ClientRpcError::new(-32603, format!("HTTP error: {e}")))?;

            let content = response.text().await.map_err(|e| {
                ClientRpcError::new(-32603, format!("Failed to read response: {e}"))
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
    /// Matches C# `RpcSend`
    pub fn rpc_send(&self, method: &str, params: Vec<JToken>) -> Result<JToken, ClientRpcError> {
        let request = Self::as_rpc_request(method, params);
        let response = self.send(request, true)?;
        response
            .result
            .ok_or_else(|| ClientRpcError::new(-32603, "No result returned".to_string()))
    }

    /// Sends an async RPC request and returns the result
    /// Matches C# `RpcSendAsync`
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
    /// Matches C# `GetRpcName`
    pub fn get_rpc_name(method_name: &str) -> String {
        let regex = RPC_NAME_REGEX.get_or_init(|| Regex::new(r"(.*?)(Hex|Both)?(Async)?").unwrap());

        regex.replace(method_name, "$1").to_lowercase()
    }

    /// Returns a list of plugins loaded by the node (matches `listplugins`).
    pub async fn get_plugins(&self) -> Result<Vec<RpcPlugin>, ClientRpcError> {
        let result = self.rpc_send_async("listplugins", vec![]).await?;
        parse_plugins(&result)
    }
}
