// Copyright (c) 2024 R3E Network
// This file is part of the neo-rs project
// Licensed under the MIT License
// See LICENSE file for details

//! # neo-rpc
//!
//! Neo JSON-RPC client, server, models, plugins, and transport adapters.
//!
//! ## Boundary
//!
//! This API crate owns JSON-RPC surfaces and transport adapters and must not
//! implement consensus, VM semantics, or storage engines.
//!
//! ## Contents
//!
//! - `application_logs`: Application-log models and retrieval helpers for RPC
//!   consumers.
//! - `plugins`: RPC plugin adapters and optional extension surfaces.
//! - `error`: Typed error definitions and conversions.
//! - `error_code`: JSON-RPC error-code records.
//! - `serialization`: serialization codecs and compatibility checks.
//! - `server`: server records and behavior.
//! - `client`: Client-side adapters for remote services and RPC access.

// ============================================================================
// Module Declarations
// ============================================================================

/// ApplicationLogs plugin for capturing execution logs.
#[cfg(feature = "server")]
pub mod application_logs;

/// Plugin implementations (merged from `neo-tokens-tracker`).
#[cfg(feature = "server")]
pub mod plugins;

/// Error types for RPC operations.
#[path = "errors/error.rs"]
pub mod error;

/// JSON-RPC error codes.
#[path = "errors/error_code.rs"]
pub mod error_code;

#[cfg(any(feature = "client", feature = "server"))]
#[path = "protocol/serialization.rs"]
mod serialization;

/// RPC server implementation (requires `server` feature).
#[cfg(feature = "server")]
pub mod server;

/// RPC client implementation (requires `client` feature).
#[cfg(feature = "client")]
pub mod client;

// ============================================================================
// Public Re-exports
// ============================================================================

// Core error types — client-side error enum renamed to RpcClientError to
// avoid collision with the server-side RpcError struct (JSON-RPC protocol
// error, matching C# Neo.Plugins.RpcServer.RpcError).
#[cfg(feature = "client")]
pub use error::{RpcClientError, RpcClientResult};
pub use error_code::RpcErrorCode;

// Server exports (requires `server` feature)
#[cfg(feature = "server")]
pub use server::{RpcServer, RpcServerConfig, RpcServerSettings};

// Client exports (requires `client` feature)
#[cfg(feature = "client")]
pub use client::{
    ClientRpcError, ContractClient, Nep17Api, PolicyApi, RpcClient, RpcClientBuilder,
    RpcClientHooks, RpcObserver, RpcRequestOutcome, RpcUtility, StateApi, TracingRpcObserver,
    TransactionManager, TransactionManagerFactory, WalletApi,
};
