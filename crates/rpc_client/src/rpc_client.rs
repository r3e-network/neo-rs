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

use crate::models::{RpcRequest, RpcResponse};
use crate::rpc_exception::RpcException;
use neo_core::ProtocolSettings;
use neo_json::JToken;
use reqwest::{Client, Url};
use regex::Regex;
use std::sync::Arc;
use std::sync::OnceLock;

static RPC_NAME_REGEX: OnceLock<Regex> = OnceLock::new();

/// The RPC client to call NEO RPC methods
/// Matches C# RpcClient
pub struct RpcClient {
    base_address: Url,
    http_client: Client,
    pub(crate) protocol_settings: Arc<ProtocolSettings>,
}

impl RpcClient {
    /// Creates a new RPC client
    /// Matches C# constructor
    pub fn new(
        url: Url,
        rpc_user: Option<String>,
        rpc_pass: Option<String>,
        protocol_settings: Option<ProtocolSettings>,
    ) -> Result<Self, Box<dyn std::error::Error>> {
        let mut builder = Client::builder();
        
        // Add basic auth if provided
        if let (Some(user), Some(pass)) = (rpc_user, rpc_pass) {
            let auth = format!("{}:{}", user, pass);
            let encoded = base64::encode(auth.as_bytes());
            builder = builder.default_headers({
                let mut headers = reqwest::header::HeaderMap::new();
                headers.insert(
                    reqwest::header::AUTHORIZATION,
                    format!("Basic {}", encoded).parse()?,
                );
                headers
            });
        }
        
        Ok(Self {
            base_address: url,
            http_client: builder.build()?,
            protocol_settings: Arc::new(protocol_settings.unwrap_or_default()),
        })
    }
    
    /// Creates a new RPC client with an existing HTTP client
    /// Matches C# constructor
    pub fn with_client(
        client: Client,
        url: Url,
        protocol_settings: Option<ProtocolSettings>,
    ) -> Self {
        Self {
            base_address: url,
            http_client: client,
            protocol_settings: Arc::new(protocol_settings.unwrap_or_default()),
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
        let json = JToken::parse(content)
            .map_err(|e| RpcException::new(-32700, format!("Parse error: {}", e)))?;
            
        let obj = json.as_object()
            .ok_or_else(|| RpcException::new(-32700, "Invalid response format".to_string()))?;
            
        let mut response = RpcResponse::from_json(obj)
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
    pub fn send(&self, request: RpcRequest, throw_on_error: bool) -> Result<RpcResponse, RpcException> {
        let runtime = tokio::runtime::Handle::try_current()
            .map_err(|_| RpcException::new(-32603, "No async runtime available".to_string()))?;
            
        runtime.block_on(self.send_async(request, throw_on_error))
    }
    
    /// Sends an asynchronous RPC request
    /// Matches C# SendAsync
    pub async fn send_async(&self, request: RpcRequest, throw_on_error: bool) -> Result<RpcResponse, RpcException> {
        let request_json = request.to_json().to_string();
        
        let response = self.http_client
            .post(self.base_address.clone())
            .header("Content-Type", "application/json")
            .body(request_json)
            .send()
            .await
            .map_err(|e| RpcException::new(-32603, format!("HTTP error: {}", e)))?;
            
        let content = response.text()
            .await
            .map_err(|e| RpcException::new(-32603, format!("Failed to read response: {}", e)))?;
            
        Self::as_rpc_response(&content, throw_on_error)
    }
    
    /// Sends an RPC request and returns the result
    /// Matches C# RpcSend
    pub fn rpc_send(&self, method: &str, params: Vec<JToken>) -> Result<JToken, RpcException> {
        let request = Self::as_rpc_request(method, params);
        let response = self.send(request, true)?;
        response.result.ok_or_else(|| RpcException::new(-32603, "No result returned".to_string()))
    }
    
    /// Sends an async RPC request and returns the result
    /// Matches C# RpcSendAsync
    pub async fn rpc_send_async(&self, method: &str, params: Vec<JToken>) -> Result<JToken, RpcException> {
        let request = Self::as_rpc_request(method, params);
        let response = self.send_async(request, true).await?;
        response.result.ok_or_else(|| RpcException::new(-32603, "No result returned".to_string()))
    }
    
    /// Gets the RPC method name from a function name
    /// Matches C# GetRpcName
    pub fn get_rpc_name(method_name: &str) -> String {
        let regex = RPC_NAME_REGEX.get_or_init(|| {
            Regex::new(r"(.*?)(Hex|Both)?(Async)?").unwrap()
        });
        
        regex.replace(method_name, "$1").to_lowercase()
    }
    
    // Blockchain methods
    
    /// Returns the hash of the tallest block in the main chain
    /// Matches C# GetBestBlockHashAsync
    pub async fn get_best_block_hash(&self) -> Result<String, RpcException> {
        let result = self.rpc_send_async("getbestblockhash", vec![]).await?;
        result.as_string()
            .ok_or_else(|| RpcException::new(-32603, "Invalid response type".to_string()))
            .map(|s| s.to_string())
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
    
    /// Returns the serialized block as hex string
    /// Matches C# GetBlockHexAsync
    pub async fn get_block_hex(&self, hash_or_index: &str) -> Result<String, RpcException> {
        let result = self.rpc_send_by_hash_or_index("getblock", hash_or_index, vec![]).await?;
        result.as_string()
            .ok_or_else(|| RpcException::new(-32603, "Invalid response type".to_string()))
            .map(|s| s.to_string())
    }
    
    /// Returns the block information
    /// Matches C# GetBlockAsync
    pub async fn get_block(&self, hash_or_index: &str) -> Result<crate::models::RpcBlock, RpcException> {
        let result = self.rpc_send_by_hash_or_index("getblock", hash_or_index, vec![JToken::Boolean(true)]).await?;
        let obj = result.as_object()
            .ok_or_else(|| RpcException::new(-32603, "Invalid response type".to_string()))?;
        crate::models::RpcBlock::from_json(obj, &self.protocol_settings)
            .map_err(|e| RpcException::new(-32603, format!("Failed to parse block: {}", e)))
    }
    
    /// Gets the number of block headers in the main chain
    /// Matches C# GetBlockHeaderCountAsync
    pub async fn get_block_header_count(&self) -> Result<u32, RpcException> {
        let result = self.rpc_send_async("getblockheadercount", vec![]).await?;
        result.as_number()
            .ok_or_else(|| RpcException::new(-32603, "Invalid response type".to_string()))
            .map(|n| n as u32)
    }
    
    /// Gets the number of blocks in the main chain
    /// Matches C# GetBlockCountAsync
    pub async fn get_block_count(&self) -> Result<u32, RpcException> {
        let result = self.rpc_send_async("getblockcount", vec![]).await?;
        result.as_number()
            .ok_or_else(|| RpcException::new(-32603, "Invalid response type".to_string()))
            .map(|n| n as u32)
    }
    
    /// Gets the hash value of the corresponding block based on the specified index
    /// Matches C# GetBlockHashAsync
    pub async fn get_block_hash(&self, index: u32) -> Result<String, RpcException> {
        let result = self.rpc_send_async("getblockhash", vec![JToken::Number(index as f64)]).await?;
        result.as_string()
            .ok_or_else(|| RpcException::new(-32603, "Invalid response type".to_string()))
            .map(|s| s.to_string())
    }
    
    /// Returns the corresponding block header information by hash or index
    /// Matches C# GetBlockHeaderHexAsync
    pub async fn get_block_header_hex(&self, hash_or_index: &str) -> Result<String, RpcException> {
        let result = self.rpc_send_by_hash_or_index("getblockheader", hash_or_index, vec![]).await?;
        result.as_string()
            .ok_or_else(|| RpcException::new(-32603, "Invalid response type".to_string()))
            .map(|s| s.to_string())
    }
    
    /// Returns the corresponding block header information by hash or index
    /// Matches C# GetBlockHeaderAsync
    pub async fn get_block_header(&self, hash_or_index: &str) -> Result<crate::models::RpcBlockHeader, RpcException> {
        let result = self.rpc_send_by_hash_or_index("getblockheader", hash_or_index, vec![JToken::Boolean(true)]).await?;
        let obj = result.as_object()
            .ok_or_else(|| RpcException::new(-32603, "Invalid response type".to_string()))?;
        crate::models::RpcBlockHeader::from_json(obj, &self.protocol_settings)
            .map_err(|e| RpcException::new(-32603, format!("Failed to parse block header: {}", e)))
    }
}
