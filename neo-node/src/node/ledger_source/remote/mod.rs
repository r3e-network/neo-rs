//! # Remote Ledger Source
//!
//! JSON-RPC-backed block source for remote-ledger mode.
//!
//! ## Boundary
//!
//! This module adapts upstream RPC payloads to the node's typed ledger-source
//! capability. It does not persist a local canonical ledger.
//!
//! ## Contents
//!
//! - `client`: synchronous bridge to the upstream RPC client.
//! - `payload`: raw Neo payload decoding.
//! - `source`: `RpcLedgerBlockSource` capability implementation.

mod client;
mod payload;
mod source;

#[cfg(test)]
#[path = "../../../tests/node/ledger_source/remote.rs"]
mod tests;

pub(in crate::node) use source::RpcLedgerBlockSource;
