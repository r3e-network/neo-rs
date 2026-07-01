//! # neo-oracle-service::tests::neofs
//!
//! Test module grouping NeoFS request signing, authentication, JSON, and
//! response helpers. coverage for neo-oracle-service.
//!
//! ## Boundary
//!
//! This is test/benchmark-only code for neo-oracle-service; it may assemble
//! fixtures but must not introduce production behavior.
//!
//! ## Contents
//!
//! - `auth`: NeoFS authentication and authorization helpers.
//! - `http`: NeoFS HTTP client helpers.
//! - `json`: JSON models and codecs for external service integration.
//! - `parse`: NeoFS response parsing helpers.

#[path = "auth.rs"]
mod auth;
#[path = "http.rs"]
mod http;
#[cfg(feature = "neofs-grpc")]
#[path = "json.rs"]
mod json;
#[path = "parse.rs"]
mod parse;
