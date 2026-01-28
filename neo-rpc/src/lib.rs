// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! # Neo RPC
//!
//! JSON-RPC server and client implementation for Neo N3.
//!
//! This crate provides a unified RPC implementation that supports both server
//! and client functionality. It implements the Neo JSON-RPC specification with
//! typed request/response models and async/await support.
//!
//! ## Architecture
//!
//! ```
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                         RPC Layer                                │
//! ├─────────────────────────────────────────────────────────────────┤
//! │                                                                  │
//! │   ┌──────────────┐         ┌──────────────┐                     │
//! │   │    Server    │         │    Client    │                     │
//! │   │              │         │              │                     │
//! │   │ • HTTP       │         │ • HTTP       │                     │
//! │   │ • WebSocket  │         │ • Typed API  │                     │
//! │   │ • Methods    │         │ • Builders   │                     │
//! │   │ • Middleware │         │ • Retry      │                     │
//! │   └──────────────┘         └──────────────┘                     │
//! │                                                                  │
//! │   ┌──────────────────────────────────────────────────────────┐  │
//! │   │              Shared Components                            │  │
//! │   │  • RpcErrorCode    • RpcError    • RpcRequest            │  │
//! │   │  • RpcResponse     • RpcVersion  • RpcBlock              │  │
//! │   └──────────────────────────────────────────────────────────┘  │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Layer Position
//!
//! This crate is part of **Layer 1 (Core)** in the neo-rs architecture:
//!
//! ```
//! Layer 2 (Service): neo-chain, neo-mempool
//!            │
//!            ▼
//! Layer 1 (Core):   neo-rpc ◄── YOU ARE HERE
//!            │
//!            ▼
//! Layer 0 (Foundation): neo-primitives, neo-io, neo-json
//! ```
//!
//! ## Features
//!
//! - `server`: Enable RPC server functionality (HTTP/WebSocket endpoints)
//! - `client`: Enable RPC client functionality (typed API, builders)
//!
//! Default: both features enabled.
//!
//! ## RPC Server
//!
//! The server implements the standard Neo JSON-RPC methods:
//!
//! ### Blockchain Methods
//!
//! | Method | Description |
//! |--------|-------------|
//! | `getbestblockhash` | Get hash of latest block |
//! | `getblock` | Get block by hash or index |
//! | `getblockcount` | Get current block height |
//! | `getblockhash` | Get block hash by index |
//! | `getblockheader` | Get block header |
//! | `getrawmempool` | Get mempool transaction list |
//! | `getrawtransaction` | Get transaction by hash |
//! | `sendrawtransaction` | Submit a transaction |
//!
//! ### Node Methods
//!
//! | Method | Description |
//! |--------|-------------|
//! | `getversion` | Get node version info |
//! | `getconnectioncount` | Get peer connection count |
//! | `getpeers` | Get peer information |
//!
//! ### Smart Contract Methods
//!
//! | Method | Description |
//! |--------|-------------|
//! | `invokefunction` | Invoke contract method (read-only) |
//! | `invokescript` | Invoke script (read-only) |
//! | `getcontractstate` | Get contract information |
//! | `getnativecontracts` | List native contracts |
//!
//! ### Wallet Methods (if wallet unlocked)
//!
//! | Method | Description |
//! |--------|-------------|
//! | `openwallet` | Open wallet file |
//! | `closewallet` | Close current wallet |
//! | `sendfrom` | Send assets from address |
//! | `sendtoaddress` | Send assets to address |
//! | `getbalance` | Get asset balance |
//!
//! ## RPC Client
//!
//! The client provides a typed, ergonomic API for interacting with Neo nodes:
//!
//! ```rust,no_run
//! use neo_rpc::{
//!     RpcClient, RpcClientBuilder,
//!     client::{Nep17Api, WalletApi},
//! };
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! // Build a client with retry and timeout
//! let client = RpcClientBuilder::new()
//!     .url("http://localhost:10332")
//!     .timeout(std::time::Duration::from_secs(30))
//!     .retry(3)
//!     .build()?;
//!
//! // Get blockchain info
//! let height = client.get_block_count().await?;
//! println!("Current height: {}", height);
//!
//! // Get block
//! let block = client.get_block_by_index(height - 1, true).await?;
//!
//! // Invoke contract (read-only)
//! let result = client.invoke_function(
//!     &contract_hash,
//!     "balanceOf",
//!     vec![account_param],
//!     None,
//! ).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Error Handling
//!
//! Errors are mapped to standard JSON-RPC error codes:
//!
//! | Code | Message | Description |
//! |------|---------|-------------|
//! | -32700 | Parse error | Invalid JSON |
//! | -32600 | Invalid Request | Invalid request object |
//! | -32601 | Method not found | Unknown method |
//! | -32602 | Invalid params | Invalid method parameters |
//! | -32603 | Internal error | Internal server error |
//! | -100 | Block not found | Requested block not found |
//! | -101 | Transaction not found | Requested transaction not found |
//! | -102 | Contract not found | Requested contract not found |
//! | -200 | Invalid address | Invalid address format |
//! | -201 | Invalid public key | Invalid public key format |
//! | -300 | Insufficient funds | Not enough balance |
//! | -400 | Wallet not open | Wallet required but not open |
//!
//! ## Example: Server Usage
//!
//! ```rust,no_run
//! use neo_rpc::server::{RpcServer, RpcServerConfig};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let config = RpcServerConfig {
//!     bind: "127.0.0.1".to_string(),
//!     port: 10332,
//!     ..Default::default()
//! };
//!
//! let server = RpcServer::new(neo_system, config);
//! server.start().await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Example: Client Usage with High-Level APIs
//!
//! ```rust,no_run
//! use neo_rpc::client::{RpcClient, Nep17Api, ContractClient};
//!
//! # async fn example() -> Result<(), Box<dyn std::error::Error>> {
//! let client = RpcClient::new("http://seed1.neo.org:10332");
//!
//! // NEP-17 token operations
//! let gas_token = client.nep17("0xd2a4cff31913016155e38e474a2c06d08be276cf");
//! let balance = gas_token.balance_of(&address).await?;
//!
//! // Contract operations
//! let contract = client.contract(&contract_hash);
//! let result = contract.call("transfer", params).await?;
//! # Ok(())
//! # }
//! ```
//!
//! ## Security Considerations
//!
//! - Always use HTTPS/TLS in production
//! - Enable authentication for wallet operations
//! - Restrict CORS origins appropriately
//! - Rate-limit requests to prevent DoS
//! - Validate all input parameters
//! - Don't expose `openwallet` on public nodes

// ============================================================================
// Module Declarations
// ============================================================================

/// Error types for RPC operations.
pub mod error;

/// JSON-RPC error codes.
pub mod error_code;

/// RPC server implementation (requires `server` feature).
#[cfg(feature = "server")]
pub mod server;

/// RPC client implementation (requires `client` feature).
#[cfg(feature = "client")]
pub mod client;

// ============================================================================
// Public Re-exports
// ============================================================================

// Core error types
pub use error::{RpcError, RpcResult};
pub use error_code::RpcErrorCode;

// Server exports (requires `server` feature)
#[cfg(feature = "server")]
pub use server::{RpcServer, RpcServerConfig, RpcServerSettings};

// Client exports (requires `client` feature)
#[cfg(feature = "client")]
pub use client::{
    ClientRpcError, ContractClient, Nep17Api, PolicyApi, RpcClient, RpcClientBuilder,
    RpcClientHooks, RpcRequestOutcome, RpcUtility, StateApi, TransactionManager,
    TransactionManagerFactory, WalletApi,
};
