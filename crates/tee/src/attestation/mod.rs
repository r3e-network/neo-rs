//! Remote attestation support for TEE verification
//!
//! Provides attestation services to prove code is running inside a genuine TEE.

mod report;
mod service;

pub use report::AttestationReport;
pub use service::AttestationService;
