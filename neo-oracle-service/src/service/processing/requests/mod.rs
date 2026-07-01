//! # neo-oracle-service::service::processing::requests
//!
//! Oracle request processing helpers and validation steps.
//!
//! ## Boundary
//!
//! This module belongs to `neo-oracle-service`. This service crate owns oracle
//! request handling and must not decide block import, consensus, or storage
//! backend policy.
//!
//! ## Contents
//!
//! - `process`: request processing steps.
//! - `submit`: oracle response submission helpers.

mod process;
mod submit;
