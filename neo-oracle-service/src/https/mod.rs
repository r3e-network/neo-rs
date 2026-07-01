//! # neo-oracle-service::https
//!
//! HTTP client and TLS helpers for oracle requests.
//!
//! ## Boundary
//!
//! This module belongs to `neo-oracle-service`. This service crate owns oracle
//! request handling and must not decide block import, consensus, or storage
//! backend policy.
//!
//! ## Contents
//!
//! - `client`: Client-side adapters for remote services and RPC access.
//! - `process`: request processing steps.
//! - `security`: oracle security validation helpers.

mod client;
mod process;
pub mod security;

pub(crate) use client::OracleHttpsProtocol;
