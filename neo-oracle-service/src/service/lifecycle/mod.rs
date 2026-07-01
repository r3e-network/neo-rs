//! # neo-oracle-service::service::lifecycle
//!
//! Service startup, shutdown, and background processing lifecycle helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-oracle-service`. This service crate owns oracle
//! request handling and must not decide block import, consensus, or storage
//! backend policy.
//!
//! ## Contents
//!
//! - `error`: Typed error definitions and conversions.
//! - `run`: service run-loop startup helpers.
//! - `state`: domain state records for the surrounding workflow.

mod error;
mod run;
mod state;
