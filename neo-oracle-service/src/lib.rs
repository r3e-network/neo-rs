//! # neo-oracle-service
//!
//! Oracle request processing, NeoFS integration, and service lifecycle helpers.
//!
//! ## Boundary
//!
//! This service crate owns oracle request handling and must not decide block
//! import, consensus, or storage backend policy.
//!
//! ## Contents
//!
//! - `https`: HTTP client and TLS helpers for oracle requests.
//! - `neofs`: NeoFS request signing, authentication, JSON, and response
//!   helpers.
//! - `service`: Service loops, handles, lifecycle helpers, and command
//!   processing.
//! - `settings`: Protocol settings, hardfork gates, and node configuration
//!   records.

#[cfg(feature = "oracle")]
mod https;
#[cfg(feature = "oracle")]
mod neofs;

pub mod service;
#[path = "config/settings.rs"]
pub mod settings;

pub use service::{OracleContractReadProvider, OracleService, OracleServiceError, OracleStatus};
pub use settings::OracleServiceSettings;
