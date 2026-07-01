//! # neo-oracle-service::neofs::json::headers
//!
//! HTTP header helpers for NeoFS JSON requests.
//!
//! ## Boundary
//!
//! This module belongs to `neo-oracle-service`. This service crate owns oracle
//! request handling and must not decide block import, consensus, or storage
//! backend policy.
//!
//! ## Contents
//!
//! - `attributes`: HTTP header attribute records.
//! - `payload`: Payload-domain primitives shared by protocol and network
//!   crates.

mod attributes;
mod payload;

pub(crate) use payload::build_neofs_header_payload;
