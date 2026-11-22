//! Attestation service for generating and verifying reports

#[allow(unused_imports)]
use crate::attestation::report::ReportType;
use crate::attestation::report::{
    AttestationReport, Quote, QuoteValidationOptions, QuoteValidationResult,
};
use crate::enclave::TeeEnclave;
use crate::error::{TeeError, TeeResult};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::Duration;
use tracing::{debug, warn};

#[cfg(not(feature = "tee-sgx"))]
use tracing::info as trace_info;

/// Enclave metadata for MRENCLAVE computation (SECS attributes)
///
/// These values correspond to the SGX Security Information (SECS) structure
/// that defines enclave properties during ECREATE.
#[derive(Debug, Clone)]
pub struct EnclaveMetadata {
    /// SSA frame size in pages (typically 1)
    pub ssa_frame_size: u64,
    /// Enclave size in bytes (must be power of 2)
    pub enclave_size: u64,
    /// MISCSELECT: Extended feature selection
    pub misc_select: u32,
    /// ATTRIBUTES: 16-byte enclave attributes
    /// - Bit 0: INIT (enclave has been initialized)
    /// - Bit 1: DEBUG (debug mode enabled)
    /// - Bit 2: MODE64BIT (64-bit mode)
    /// - Bit 4: PROVISIONKEY (provisioning key available)
    /// - Bit 5: EINITTOKEN_KEY (EINIT token key available)
    pub attributes: [u8; 16],
    /// Page type for EADD operations:
    /// - 0x01: REG (regular page)
    /// - 0x02: TCS (thread control structure)
    /// - 0x04: TRIM (trim page)
    /// Lower 8 bits: type, upper 8 bits: permissions (R=1, W=2, X=4)
    pub page_type: u64,
}

impl Default for EnclaveMetadata {
    fn default() -> Self {
        Self {
            ssa_frame_size: 1,               // 1 page (4096 bytes)
            enclave_size: 1024 * 1024 * 128, // 128 MB default
            misc_select: 0,
            attributes: [
                0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // MODE64BIT
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ],
            page_type: 0x0501, // REG page with R+X permissions
        }
    }
}

impl EnclaveMetadata {
    /// Create metadata for production enclave (non-debug, 64-bit)
    pub fn production(enclave_size: u64) -> Self {
        Self {
            ssa_frame_size: 1,
            enclave_size,
            misc_select: 0,
            attributes: [
                0x04, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // MODE64BIT only
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ],
            page_type: 0x0501, // REG + R+X
        }
    }

    /// Create metadata for debug enclave
    pub fn debug(enclave_size: u64) -> Self {
        Self {
            ssa_frame_size: 1,
            enclave_size,
            misc_select: 0,
            attributes: [
                0x06, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, // DEBUG + MODE64BIT
                0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00,
            ],
            page_type: 0x0501,
        }
    }
}

/// Configuration for attestation verification
#[derive(Debug, Clone)]
pub struct AttestationConfig {
    /// Expected MRENCLAVE (if None, skips MRENCLAVE verification)
    pub expected_mrenclave: Option<[u8; 32]>,
    /// Expected MRSIGNER (if None, skips MRSIGNER verification)
    pub expected_mrsigner: Option<[u8; 32]>,
    /// Minimum acceptable ISV SVN
    pub min_isv_svn: u16,
    /// Maximum age of a valid attestation report
    pub max_report_age: Duration,
    /// Require non-debug enclaves
    pub require_non_debug: bool,
    /// Allow simulated reports (for testing)
    pub allow_simulated: bool,
}

impl Default for AttestationConfig {
    fn default() -> Self {
        Self {
            expected_mrenclave: None,
            expected_mrsigner: None,
            min_isv_svn: 1,
            max_report_age: Duration::from_secs(24 * 60 * 60), // 24 hours
            require_non_debug: false,
            allow_simulated: false,
        }
    }
}

impl AttestationConfig {
    /// Create a strict configuration for production use
    pub fn production() -> Self {
        Self {
            expected_mrenclave: None, // Must be set explicitly
            expected_mrsigner: None,  // Should be set explicitly
            min_isv_svn: 1,
            max_report_age: Duration::from_secs(60 * 60), // 1 hour
            require_non_debug: true,
            allow_simulated: false,
        }
    }

    /// Create a permissive configuration for testing
    pub fn testing() -> Self {
        Self {
            expected_mrenclave: None,
            expected_mrsigner: None,
            min_isv_svn: 1,
            max_report_age: Duration::from_secs(7 * 24 * 60 * 60), // 7 days
            require_non_debug: false,
            allow_simulated: true,
        }
    }

    /// Set expected MRENCLAVE
    pub fn with_mrenclave(mut self, mrenclave: [u8; 32]) -> Self {
        self.expected_mrenclave = Some(mrenclave);
        self
    }

    /// Set expected MRSIGNER
    pub fn with_mrsigner(mut self, mrsigner: [u8; 32]) -> Self {
        self.expected_mrsigner = Some(mrsigner);
        self
    }

    /// Convert to quote validation options
    fn to_quote_options(&self) -> QuoteValidationOptions {
        QuoteValidationOptions {
            expected_mrenclave: self.expected_mrenclave,
            expected_mrsigner: self.expected_mrsigner,
            min_isv_svn: self.min_isv_svn,
            max_age: self.max_report_age,
            require_non_debug: self.require_non_debug,
        }
    }
}

/// Service for generating and verifying attestation reports
pub struct AttestationService {
    _enclave: Arc<TeeEnclave>,
    config: AttestationConfig,
}

impl AttestationService {
    /// Create a new attestation service with default configuration
    pub fn new(enclave: Arc<TeeEnclave>) -> TeeResult<Self> {
        Self::with_config(enclave, AttestationConfig::default())
    }

    /// Create a new attestation service with custom configuration
    pub fn with_config(enclave: Arc<TeeEnclave>, config: AttestationConfig) -> TeeResult<Self> {
        if !enclave.is_ready() {
            return Err(TeeError::EnclaveNotInitialized);
        }

        Ok(Self {
            _enclave: enclave,
            config,
        })
    }

    /// Get the attestation configuration
    pub fn config(&self) -> &AttestationConfig {
        &self.config
    }
}

/// Error returned when cryptographic quote verification is not compiled in.
///
/// C-2: structural quote checks are not verification. Without the `tee-sgx`
/// feature the crate cannot authenticate a quote, so the quote verification
/// entry points fail closed with this error instead of returning `Valid`.
#[cfg(not(feature = "tee-sgx"))]
fn hardware_verification_unavailable() -> TeeError {
    TeeError::FeatureNotEnabled(
        "hardware quote verification unavailable: neo-tee was built without the `tee-sgx` feature"
            .to_string(),
    )
}

mod measure;
mod report;
mod verify;

#[cfg(test)]
mod tests;
