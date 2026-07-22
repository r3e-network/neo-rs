//! # neo-rpc::protocol
//!
//! Shared Neo JSON-RPC protocol codecs used by both transports.
//!
//! ## Boundary
//!
//! This module owns deterministic text and JSON conversion mechanics. It does
//! not perform HTTP requests, dispatch server methods, or read node state.
//!
//! ## Contents
//!
//! - `address`: Neo address and script-hash text decoding.

pub(crate) mod address;
