//! # neo-rpc::server::rpc_remote_ledger
//!
//! Remote-ledger RPC client used by RPC-only node mode.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `client`: remote-ledger JSON-RPC client facade.
//! - `policy`: remote-ledger proxy method catalog.
//! - `transport`: blocking HTTP transport for remote-ledger JSON-RPC calls.

mod client;
mod policy;
mod transport;

pub use client::RemoteLedgerRpcClient;
pub(super) use policy::should_proxy_remote_ledger_method;
