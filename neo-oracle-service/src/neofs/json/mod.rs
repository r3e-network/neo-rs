//! # neo-oracle-service::neofs::json
//!
//! JSON models and codecs for external service integration.
//!
//! ## Boundary
//!
//! This module belongs to `neo-oracle-service`. This service crate owns oracle
//! request handling and must not decide block import, consensus, or storage
//! backend policy.
//!
//! ## Contents
//!
//! - `headers`: HTTP header helpers for NeoFS JSON requests.
//! - `helpers`: Shared helper functions for the surrounding module.
//! - `object`: NeoFS JSON object models.
//! - `session`: NeoFS JSON session token models.

#![allow(dead_code)]

mod headers;
mod helpers;
mod object;
mod session;

pub(crate) use headers::build_neofs_header_payload;
pub(crate) use helpers::normalize_neofs_hex_header;
#[cfg(feature = "neofs-grpc")]
#[allow(unused)]
pub(crate) use object::{
    build_neofs_object_payload, neofs_json_header, neofs_json_object_id, neofs_json_version,
};
#[cfg(feature = "neofs-grpc")]
#[allow(unused)]
pub(crate) use session::neofs_json_session_token;
