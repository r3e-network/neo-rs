//! Oracle service implementation for Neo N3 nodes.
//!
//! This crate is the authoritative home of the Oracle service (parity with
//! `Neo.Plugins.OracleService`), including:
//! - HTTPS request processing with SSRF protection
//! - NeoFS object retrieval over REST and (under `neofs-grpc`) gRPC
//! - JSONPath filter execution
//! - Request deduplication and completion tracking
//! - Response signature aggregation
//! - Lifecycle management (start/stop)
//!
//! # Features
//!
//! - `neofs-grpc`: enables the NeoFS gRPC transport, request signing and
//!   response verification path. Mirrors neo-core's `neofs-grpc` feature.
//! - `full`: aggregate gate enabling every optional capability.
//!
//! # Integration
//!
//! The service is generic over infrastructure traits defined in [`ports`].
//! Concrete implementations bridge to the ledger, native contracts and
//! actor system of the host node.
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────┐
//! │                     OracleService<S,W,Sig,TB,R,V>           │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
//! │  │  Settings   │  │   Dedup     │  │   Pending Queue     │ │
//! │  │  (config)   │  │  (in-flight │  │  (signature aggr)   │ │
//! │  │             │  │   tracking) │  │                     │ │
//! │  └─────────────┘  └─────────────┘  └─────────────────────┘ │
//! │  ┌─────────────┐  ┌─────────────┐  ┌─────────────────────┐ │
//! │  │   HTTPS     │  │   Filter    │  │    Security         │ │
//! │  │  (reqwest)  │  │  (JSONPath) │  │  (SSRF protection)  │ │
//! │  └─────────────┘  └─────────────┘  └─────────────────────┘ │
//! │  ┌─────────────┐                                            │
//! │  │    NeoFS    │  REST + gRPC object retrieval              │
//! │  └─────────────┘                                            │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼
//! ┌─────────────────────────────────────────────────────────────┐
//! │                    Port Traits (ports.rs)                   │
//! │  OracleSnapshot │ OracleWallet │ OracleSigner │ TxBuilder   │
//! │  TxRelay │ SignatureVerifier │ SnapshotFactory              │
//! └─────────────────────────────────────────────────────────────┘
//!                              │
//!                              ▼ (host-node integration)
//! ┌─────────────────────────────────────────────────────────────┐
//! │         ledger state, native contracts, actor system        │
//! └─────────────────────────────────────────────────────────────┘
//! ```

pub mod dedup;
pub mod error;
pub mod filter;
pub mod https;
pub mod neofs;
pub mod ports;
pub mod request;
pub mod security;
pub mod service;
pub mod settings;

// Re-export primary types
pub use dedup::{
    DEDUP_CACHE_TTL, ExpiringSet, ExpiryBoundary, FINISHED_CACHE_TTL, OracleDedupState,
};
pub use error::OracleServiceError;
pub use filter::{FILTER_MAX_NEST, filter_json};
pub use https::OracleHttpsProtocol;
pub use neofs::OracleNeoFsProtocol;
pub use ports::{
    OracleInfrastructure, OracleSigner, OracleSnapshot, OracleWallet, ResponseTxBuilder,
    SignatureVerifier, SnapshotFactory, TxRelay,
};
pub use request::OracleRequest;
pub use security::{
    is_internal_ip, is_localhost_name, validate_url_for_ssrf, validated_external_addresses,
};
pub use service::{MAX_PENDING_QUEUE_SIZE, MAX_TASK_PENDING_TIME, OracleService, OracleStatus};
pub use settings::{
    DEFAULT_ORACLE_REQUEST_TIMEOUT, MAX_ORACLE_RESPONSE_SIZE, OracleServiceSettings,
};
