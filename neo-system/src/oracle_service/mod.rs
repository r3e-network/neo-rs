//! Oracle service module (parity with Neo.Plugins.OracleService).
//!
//! The authoritative engine (request/timer loops, URL dispatch, dedup,
//! signature aggregation, HTTPS/NeoFS transports) lives in the `neo-oracle`
//! crate. This module keeps a thin shell ([`OracleService`]) composed with
//! production adapters ([`adapters`]) plus a settings re-export, preserving
//! the historical neo-core API for consumers (neo-node startup, neo-rpc).
//!
//! The NeoFS transport (REST + gRPC, bearer auth, request signing and response
//! verification) is owned by the `neo-oracle` crate. The host wallet adapter
//! supplies only real unlocked Oracle keys; unavailable hardware/remote key
//! export remains fail-closed.

mod adapters;

pub mod service;
pub mod settings;

pub use neo_wallets::signing::{
    FrozenSignIntent, KeyRef, ProtocolContext, SIGNING_SCHEMA_VERSION, SignError as SigningError,
    SignPayload, SignPurpose, SignResponse, TrustedSignerClient,
};
pub use service::{OracleService, OracleServiceError, OracleStatus};
pub use settings::OracleServiceSettings;
