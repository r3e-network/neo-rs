//! RPC Service for neo-node runtime
//!
//! This module provides a lightweight RPC service wrapper that integrates
//! with the node runtime. It provides basic JSON-RPC endpoints for node status.

use hyper::service::{make_service_fn, service_fn};
use hyper::{Body, Method, Request, Response, Server, StatusCode};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};
use tracing::{error, info};

use neo_core::protocol_settings::ProtocolSettings;
use neo_core::wallets::helper::Helper as WalletHelper;

/// RPC service configuration
#[derive(Debug, Clone)]
pub struct RpcServiceConfig {
    /// Bind address
    pub bind_address: SocketAddr,
    /// Enable CORS
    pub cors_enabled: bool,
    /// Allowed origins for CORS
    pub allowed_origins: Vec<String>,
}

impl Default for RpcServiceConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1:10332".parse().unwrap(),
            cors_enabled: true,
            allowed_origins: vec!["*".to_string()],
        }
    }
}

/// RPC service state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcServiceState {
    Stopped,
    Running,
}

/// Node status for RPC responses
#[derive(Debug, Clone, Serialize)]
#[allow(dead_code)] // Will be used when full RPC status endpoint is implemented
pub struct NodeStatus {
    pub height: u32,
    pub peer_count: usize,
    pub mempool_size: usize,
    pub version: String,
    pub network_magic: u32,
}

/// Shared state for RPC handlers
#[allow(dead_code)] // Fields will be used when full RPC endpoints are implemented
pub struct RpcState {
    pub height: u32,
    pub peer_count: usize,
    pub mempool_size: usize,
    pub mempool_hashes: Vec<String>,
    pub peers_connected: Vec<(String, u16)>,
    pub version: String,
    pub network_magic: u32,
    pub tcp_port: u16,
    pub nonce: u32,
    pub user_agent: String,
    pub protocol_settings: Option<ProtocolSettings>,
    pub rpc_max_iterator_result_items: usize,
    pub rpc_session_enabled: bool,
}

impl Default for RpcState {
    fn default() -> Self {
        Self {
            height: 0,
            peer_count: 0,
            mempool_size: 0,
            mempool_hashes: Vec::new(),
            peers_connected: Vec::new(),
            version: env!("CARGO_PKG_VERSION").to_string(),
            network_magic: 0,
            tcp_port: 0,
            nonce: 0,
            user_agent: "/neo-rs/".to_string(),
            protocol_settings: None,
            rpc_max_iterator_result_items: 100,
            rpc_session_enabled: false,
        }
    }
}

/// JSON-RPC request
#[derive(Debug, Deserialize)]
#[allow(dead_code)] // Fields used during JSON deserialization
struct JsonRpcRequest {
    jsonrpc: String,
    method: String,
    params: Option<serde_json::Value>,
    id: serde_json::Value,
}

/// JSON-RPC response
#[derive(Debug, Serialize)]
struct JsonRpcResponse {
    jsonrpc: String,
    result: Option<serde_json::Value>,
    error: Option<JsonRpcError>,
    id: serde_json::Value,
}

/// JSON-RPC error
#[derive(Debug, Serialize)]
struct JsonRpcError {
    code: i32,
    message: String,
}

/// RPC Service
#[derive(Clone)]
pub struct RpcService {
    config: RpcServiceConfig,
    state: Arc<RwLock<RpcServiceState>>,
    rpc_state: Arc<RwLock<RpcState>>,
    shutdown_tx: broadcast::Sender<()>,
}

impl RpcService {
    /// Creates a new RPC service
    pub fn new(config: RpcServiceConfig) -> Self {
        let (shutdown_tx, _) = broadcast::channel(8);

        Self {
            config,
            state: Arc::new(RwLock::new(RpcServiceState::Stopped)),
            rpc_state: Arc::new(RwLock::new(RpcState::default())),
            shutdown_tx,
        }
    }

    /// Returns the current service state
    pub async fn state(&self) -> RpcServiceState {
        *self.state.read().await
    }

    /// Updates the RPC state
    pub async fn update_state(&self, height: u32, peer_count: usize, mempool_size: usize) {
        let mut state = self.rpc_state.write().await;
        state.height = height;
        state.peer_count = peer_count;
        state.mempool_size = mempool_size;
    }

    /// Updates the RPC state with mempool transaction hashes
    pub async fn update_state_with_mempool(
        &self,
        height: u32,
        peer_count: usize,
        mempool_hashes: Vec<String>,
    ) {
        let mut state = self.rpc_state.write().await;
        state.height = height;
        state.peer_count = peer_count;
        state.mempool_size = mempool_hashes.len();
        state.mempool_hashes = mempool_hashes;
    }

    /// Sets the network magic
    pub async fn set_network_magic(&self, magic: u32) {
        self.rpc_state.write().await.network_magic = magic;
    }

    /// Sets the local node metadata (tcp port, node nonce, user agent).
    pub async fn set_node_info(&self, tcp_port: u16, nonce: u32, user_agent: String) {
        let mut state = self.rpc_state.write().await;
        state.tcp_port = tcp_port;
        state.nonce = nonce;
        state.user_agent = user_agent;
    }

    /// Sets the protocol + RPC settings used by `getversion`.
    pub async fn set_version_settings(
        &self,
        protocol_settings: ProtocolSettings,
        rpc_max_iterator_result_items: usize,
        rpc_session_enabled: bool,
    ) {
        let mut state = self.rpc_state.write().await;
        state.protocol_settings = Some(protocol_settings);
        state.rpc_max_iterator_result_items = rpc_max_iterator_result_items;
        state.rpc_session_enabled = rpc_session_enabled;
    }

    /// Updates the peer endpoints returned by `getpeers`.
    pub async fn update_peers_connected(&self, peers: Vec<(String, u16)>) {
        self.rpc_state.write().await.peers_connected = peers;
    }

    /// Starts the RPC service
    pub async fn start(&self) -> anyhow::Result<()> {
        {
            let mut state = self.state.write().await;
            if *state != RpcServiceState::Stopped {
                anyhow::bail!("RPC service is already running");
            }
            *state = RpcServiceState::Running;
        }

        let addr = self.config.bind_address;
        let rpc_state = self.rpc_state.clone();
        let cors_enabled = self.config.cors_enabled;
        let mut shutdown_rx = self.shutdown_tx.subscribe();

        info!(target: "neo::rpc", address = %addr, "starting RPC service");

        tokio::spawn(async move {
            let make_svc = make_service_fn(move |_conn| {
                let rpc_state = rpc_state.clone();
                async move {
                    Ok::<_, hyper::Error>(service_fn(move |req| {
                        let rpc_state = rpc_state.clone();
                        async move { handle_request(req, rpc_state, cors_enabled).await }
                    }))
                }
            });

            let server = Server::bind(&addr).serve(make_svc);

            tokio::select! {
                result = server => {
                    if let Err(e) = result {
                        error!(target: "neo::rpc", error = %e, "RPC server error");
                    }
                }
                _ = shutdown_rx.recv() => {
                    info!(target: "neo::rpc", "RPC service shutting down");
                }
            }
        });

        info!(target: "neo::rpc", "RPC service started");
        Ok(())
    }

    /// Stops the RPC service
    pub async fn stop(&self) -> anyhow::Result<()> {
        {
            let mut state = self.state.write().await;
            if *state != RpcServiceState::Running {
                return Ok(());
            }
            *state = RpcServiceState::Stopped;
        }

        let _ = self.shutdown_tx.send(());
        info!(target: "neo::rpc", "RPC service stopped");
        Ok(())
    }
}

/// Handles an HTTP request
async fn handle_request(
    req: Request<Body>,
    rpc_state: Arc<RwLock<RpcState>>,
    cors_enabled: bool,
) -> Result<Response<Body>, hyper::Error> {
    // Handle CORS preflight
    if req.method() == Method::OPTIONS {
        return Ok(cors_response(cors_enabled));
    }

    // Only accept POST for JSON-RPC
    if req.method() != Method::POST {
        return Ok(Response::builder()
            .status(StatusCode::METHOD_NOT_ALLOWED)
            .body(Body::from("Method not allowed"))
            .unwrap());
    }

    // Parse request body
    let body_bytes = hyper::body::to_bytes(req.into_body()).await?;
    let parsed: serde_json::Value = match serde_json::from_slice(&body_bytes) {
        Ok(v) => v,
        Err(e) => {
            let response = JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32700,
                    message: format!("Parse error: {}", e),
                }),
                id: serde_json::Value::Null,
            };
            let json = serde_json::to_string(&response).unwrap_or_else(|_| "{}".to_string());
            let mut resp = Response::new(Body::from(json));
            resp.headers_mut()
                .insert("Content-Type", "application/json".parse().unwrap());
            if cors_enabled {
                resp.headers_mut()
                    .insert("Access-Control-Allow-Origin", "*".parse().unwrap());
            }
            return Ok(resp);
        }
    };

    let json = match parsed {
        serde_json::Value::Array(items) => {
            if items.is_empty() {
                serde_json::to_string(&JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32600,
                        message: "Invalid Request".to_string(),
                    }),
                    id: serde_json::Value::Null,
                })
                .unwrap_or_else(|_| "{}".to_string())
            } else {
                let mut responses = Vec::with_capacity(items.len());
                for item in items {
                    match serde_json::from_value::<JsonRpcRequest>(item) {
                        Ok(rpc_req) => {
                            responses.push(handle_rpc_request(rpc_req, rpc_state.clone()).await)
                        }
                        Err(_) => responses.push(JsonRpcResponse {
                            jsonrpc: "2.0".to_string(),
                            result: None,
                            error: Some(JsonRpcError {
                                code: -32600,
                                message: "Invalid Request".to_string(),
                            }),
                            id: serde_json::Value::Null,
                        }),
                    }
                }
                serde_json::to_string(&responses).unwrap_or_else(|_| "[]".to_string())
            }
        }
        other => match serde_json::from_value::<JsonRpcRequest>(other) {
            Ok(rpc_req) => serde_json::to_string(&handle_rpc_request(rpc_req, rpc_state).await)
                .unwrap_or_else(|_| "{}".to_string()),
            Err(_) => serde_json::to_string(&JsonRpcResponse {
                jsonrpc: "2.0".to_string(),
                result: None,
                error: Some(JsonRpcError {
                    code: -32600,
                    message: "Invalid Request".to_string(),
                }),
                id: serde_json::Value::Null,
            })
            .unwrap_or_else(|_| "{}".to_string()),
        },
    };

    let mut resp = Response::new(Body::from(json));
    resp.headers_mut()
        .insert("Content-Type", "application/json".parse().unwrap());

    if cors_enabled {
        resp.headers_mut()
            .insert("Access-Control-Allow-Origin", "*".parse().unwrap());
    }

    Ok(resp)
}

/// Handles a JSON-RPC request
async fn handle_rpc_request(
    req: JsonRpcRequest,
    rpc_state: Arc<RwLock<RpcState>>,
) -> JsonRpcResponse {
    let state = rpc_state.read().await;

    let result = match req.method.as_str() {
        "getversion" => {
            let Some(settings) = state.protocol_settings.as_ref() else {
                return JsonRpcResponse {
                    jsonrpc: "2.0".to_string(),
                    result: None,
                    error: Some(JsonRpcError {
                        code: -32603,
                        message: "Protocol settings not initialised".to_string(),
                    }),
                    id: req.id,
                };
            };

            let mut hardforks: Vec<_> = settings.hardforks.iter().collect();
            hardforks.sort_by_key(|(hf, _height)| hf.index());

            let version_info = serde_json::json!({
                "tcpport": state.tcp_port,
                "nonce": state.nonce,
                "useragent": state.user_agent,
                "rpc": {
                    "maxiteratorresultitems": state.rpc_max_iterator_result_items,
                    "sessionenabled": state.rpc_session_enabled
                },
                "protocol": {
                    "addressversion": settings.address_version,
                    "network": settings.network,
                    "validatorscount": settings.validators_count,
                    "msperblock": settings.milliseconds_per_block,
                    "maxtraceableblocks": settings.max_traceable_blocks,
                    "maxvaliduntilblockincrement": settings.max_valid_until_block_increment,
                    "maxtransactionsperblock": settings.max_transactions_per_block,
                    "memorypoolmaxtransactions": settings.memory_pool_max_transactions,
                    "initialgasdistribution": settings.initial_gas_distribution,
                    "hardforks": hardforks.iter().map(|(hf, height)| {
                        let name = hf.to_string();
                        serde_json::json!({
                            "name": name.trim_start_matches("HF_").to_string(),
                            "blockheight": *height
                        })
                    }).collect::<Vec<_>>(),
                    "standbycommittee": settings.standby_committee.iter().map(|k| k.to_string()).collect::<Vec<_>>(),
                    "seedlist": settings.seed_list.clone()
                }
            });

            Ok(version_info)
        }
        "getblockcount" => Ok(serde_json::json!(state.height + 1)),
        "getconnectioncount" => Ok(serde_json::json!(state.peer_count)),
        "getrawmempool" => Ok(serde_json::json!(state.mempool_hashes)),
        "getpeers" => Ok(serde_json::json!({
            "unconnected": [],
            "bad": [],
            "connected": state.peers_connected.iter().map(|(addr, port)| serde_json::json!({
                "address": addr,
                "port": *port
            })).collect::<Vec<_>>()
        })),
        "validateaddress" => {
            if let Some(params) = &req.params {
                if let Some(address) = params.get(0).and_then(|v| v.as_str()) {
                    let settings = state.protocol_settings.as_ref();
                    let version = settings.map(|s| s.address_version).unwrap_or(0x35);
                    let is_valid = WalletHelper::to_script_hash(address, version).is_ok();
                    Ok(serde_json::json!({
                        "address": address,
                        "isvalid": is_valid
                    }))
                } else {
                    Err((-32602, "Invalid params"))
                }
            } else {
                Err((-32602, "Missing params"))
            }
        }
        _ => Err((-32601, "Method not found")),
    };

    match result {
        Ok(value) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: Some(value),
            error: None,
            id: req.id,
        },
        Err((code, message)) => JsonRpcResponse {
            jsonrpc: "2.0".to_string(),
            result: None,
            error: Some(JsonRpcError {
                code,
                message: message.to_string(),
            }),
            id: req.id,
        },
    }
}

/// Creates a CORS preflight response
fn cors_response(enabled: bool) -> Response<Body> {
    let mut resp = Response::new(Body::empty());
    *resp.status_mut() = StatusCode::NO_CONTENT;

    if enabled {
        resp.headers_mut()
            .insert("Access-Control-Allow-Origin", "*".parse().unwrap());
        resp.headers_mut().insert(
            "Access-Control-Allow-Methods",
            "POST, OPTIONS".parse().unwrap(),
        );
        resp.headers_mut().insert(
            "Access-Control-Allow-Headers",
            "Content-Type".parse().unwrap(),
        );
    }

    resp
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_rpc_service_creation() {
        let config = RpcServiceConfig::default();
        let service = RpcService::new(config);

        assert_eq!(service.state().await, RpcServiceState::Stopped);
    }

    #[tokio::test]
    async fn test_rpc_state_update() {
        let config = RpcServiceConfig::default();
        let service = RpcService::new(config);

        service.update_state(100, 5, 10).await;

        let state = service.rpc_state.read().await;
        assert_eq!(state.height, 100);
        assert_eq!(state.peer_count, 5);
        assert_eq!(state.mempool_size, 10);
    }
}
