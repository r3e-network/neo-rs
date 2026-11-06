//! Oracle service settings re-export.
//!
//! The authoritative definition lives in `neo-oracle`
//! ([`neo_oracle::settings`]); it was moved there together with the engine.
//! This module preserves the historical
//! `neo_system::oracle_service::settings` path for existing consumers.

pub use neo_oracle::settings::{
    DEFAULT_ORACLE_REQUEST_TIMEOUT, MAX_ORACLE_RESPONSE_SIZE, OracleServiceSettings,
};
