//! Oracle service module (parity with Neo.Plugins.OracleService).
//!
//! This module implements oracle request processing, signature aggregation,
//! and RPC submission support for oracle nodes.

#[cfg(feature = "oracle")]
mod https;
#[cfg(feature = "oracle")]
mod neofs;

pub mod service;
pub mod settings;

pub use service::{OracleService, OracleServiceError, OracleStatus};
pub use settings::OracleServiceSettings;
