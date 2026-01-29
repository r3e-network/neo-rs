//! Remote attestation support for TEE verification
//!
//! Provides attestation services to prove code is running inside a genuine TEE.

mod report;
mod service;

pub use report::{
    AttestationReport, EnclaveAttributes, Quote, QuoteValidationOptions, QuoteValidationResult,
    ReportType, MAX_REPORT_AGE_SECONDS, MIN_SECURITY_VERSION,
};
pub use service::{AttestationConfig, AttestationService};
