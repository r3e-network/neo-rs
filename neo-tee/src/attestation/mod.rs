//! Remote attestation support for TEE verification
//!
//! Provides attestation services to prove code is running inside a genuine TEE.

mod report;
mod service;

pub use report::{
    AttestationReport, EnclaveAttributes, MAX_REPORT_AGE_SECONDS, MIN_SECURITY_VERSION, Quote,
    QuoteValidationOptions, QuoteValidationResult, ReportType,
};
pub use service::{AttestationConfig, AttestationService};
