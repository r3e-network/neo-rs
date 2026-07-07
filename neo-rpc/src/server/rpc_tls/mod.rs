//! # neo-rpc::server::rpc_tls
//!
//! TLS configuration helpers for RPC transports.
//!
//! ## Boundary
//!
//! This module belongs to `neo-rpc`. This API crate owns JSON-RPC surfaces and
//! transport adapters and must not implement consensus, VM semantics, or
//! storage engines.
//!
//! ## Contents
//!
//! - `authorities`: trusted client-certificate root filtering by thumbprint.
//! - `certificate`: PKCS#12 server identity loading.
//! - `config`: `rustls::ServerConfig` construction from RPC settings.

mod authorities;
mod certificate;
mod config;

pub use config::build_tls_config_from_settings;
