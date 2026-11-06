//! Oracle service module (parity with Neo.Plugins.OracleService).
//!
//! The authoritative engine (request/timer loops, URL dispatch, dedup,
//! signature aggregation, HTTPS/NeoFS transports) lives in the `neo-oracle`
//! crate. This module keeps a thin shell ([`OracleService`]) composed with
//! production adapters ([`adapters`]) plus a settings re-export, preserving
//! the historical neo-core API for consumers (neo-node startup, neo-rpc).
//!
//! The NeoFS transport (REST + gRPC, bearer auth, request signing and response
//! verification) is owned by the `neo-oracle` crate. It is not re-exported
//! here because `neofs:` oracle requests fail closed in the engine until the
//! dedicated NeoFS signer algorithms land, so neo-core carries no NeoFS code.

mod adapters;

pub mod service;
pub mod settings;

pub use service::{OracleService, OracleServiceError, OracleStatus};
pub use settings::OracleServiceSettings;
