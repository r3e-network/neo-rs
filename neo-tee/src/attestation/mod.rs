//! # neo-tee::attestation
//!
//! TEE attestation evidence and verification helpers.
//!
//! ## Boundary
//!
//! This module belongs to `neo-tee`. This adapter crate owns TEE integration
//! and must not define protocol bytes, consensus rules, or storage semantics.
//!
//! ## Contents
//!
//! - `report`: TEE attestation report records.
//! - `service`: Service loops, handles, lifecycle helpers, and command
//!   processing.

mod report;
mod service;

pub use report::{
    AttestationReport, EnclaveAttributes, MAX_REPORT_AGE_SECONDS, MIN_SECURITY_VERSION, Quote,
    QuoteValidationOptions, QuoteValidationResult, ReportType,
};
pub use service::{AttestationConfig, AttestationService};
