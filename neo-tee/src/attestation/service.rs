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

#[cfg(not(feature = "sgx-hw"))]
use tracing::info as trace_info;

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
            allow_simulated: cfg!(feature = "simulation"),
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

    /// Generate an attestation report with custom report data
    pub fn generate_report(&self, user_data: &[u8]) -> TeeResult<AttestationReport> {
        #[cfg(feature = "sgx-hw")]
        let report_data = crate::sgx::report_data_from_user_data(user_data);

        #[cfg(not(feature = "sgx-hw"))]
        let report_data = {
            let mut data = [0u8; 64];
            let len = user_data.len().min(64);
            data[..len].copy_from_slice(&user_data[..len]);
            data
        };

        #[cfg(feature = "sgx-hw")]
        {
            self.generate_sgx_report(report_data)
        }

        #[cfg(not(feature = "sgx-hw"))]
        {
            self.generate_simulated_report(report_data)
        }
    }

    /// Generate a report binding to a specific public key (for key attestation)
    pub fn attest_key(&self, public_key: &[u8]) -> TeeResult<AttestationReport> {
        // Hash the public key to fit in report data
        let mut hasher = Sha256::new();
        hasher.update(b"neo-tee-key-attestation");
        hasher.update(public_key);
        let hash = hasher.finalize();

        let mut report_data = [0u8; 64];
        report_data[..32].copy_from_slice(&hash);

        // Include timestamp in second half
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        report_data[32..40].copy_from_slice(&timestamp.to_le_bytes());

        self.generate_report(&report_data)
    }

    /// Generate a report for mempool ordering proof
    pub fn attest_ordering(
        &self,
        merkle_root: &[u8; 32],
        batch_id: u64,
    ) -> TeeResult<AttestationReport> {
        let mut report_data = [0u8; 64];
        report_data[..32].copy_from_slice(merkle_root);
        report_data[32..40].copy_from_slice(&batch_id.to_le_bytes());

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();
        report_data[40..48].copy_from_slice(&timestamp.to_le_bytes());

        self.generate_report(&report_data)
    }

    /// Verify an attestation report with full validation
    pub fn verify_report(&self, report: &AttestationReport) -> TeeResult<bool> {
        // Check if simulated reports are allowed
        if report.report_type == ReportType::Simulated && !self.config.allow_simulated {
            warn!("Simulated attestation reports are not allowed");
            return Ok(false);
        }

        // Basic validation
        if report.version == 0 {
            debug!("Invalid report version: 0");
            return Ok(false);
        }

        // Perform full validation with options
        let options = self.config.to_quote_options();
        let result = report.validate(&options);

        if result != QuoteValidationResult::Valid {
            warn!(
                "Attestation report validation failed: {}",
                result.description()
            );
            return Ok(false);
        }

        debug!("Attestation report verified successfully");
        Ok(true)
    }

    /// Verify an attestation report and return detailed result
    pub fn verify_report_detailed(&self, report: &AttestationReport) -> QuoteValidationResult {
        // Check if simulated reports are allowed
        if report.report_type == ReportType::Simulated && !self.config.allow_simulated {
            return QuoteValidationResult::InvalidSignature;
        }

        let options = self.config.to_quote_options();
        report.validate(&options)
    }

    /// Verify that a report matches expected enclave measurements
    pub fn verify_enclave_identity(
        &self,
        report: &AttestationReport,
        expected_mrenclave: Option<&[u8; 32]>,
        expected_mrsigner: Option<&[u8; 32]>,
    ) -> bool {
        if let Some(expected) = expected_mrenclave {
            if !report.verify_mrenclave(expected) {
                debug!("MRENCLAVE mismatch");
                return false;
            }
        }

        if let Some(expected) = expected_mrsigner {
            if !report.verify_mrsigner(expected) {
                debug!("MRSIGNER mismatch");
                return false;
            }
        }

        true
    }

    /// Verify a quote from raw bytes
    pub fn verify_quote(&self, quote_bytes: &[u8]) -> TeeResult<QuoteValidationResult> {
        let quote = match Quote::from_bytes(quote_bytes) {
            Some(q) => q,
            None => {
                return Err(TeeError::InvalidAttestationReport(
                    "Failed to parse quote from bytes".to_string(),
                ));
            }
        };

        let options = self.config.to_quote_options();
        let result = quote.validate(&options);
        if result != QuoteValidationResult::Valid {
            return Ok(result);
        }

        #[cfg(feature = "sgx-hw")]
        {
            if let Err(err) = crate::sgx::verify_quote_signature(quote_bytes) {
                warn!("SGX DCAP quote verification failed: {}", err);
                return Ok(QuoteValidationResult::InvalidSignature);
            }
        }

        Ok(QuoteValidationResult::Valid)
    }

    /// Verify a quote with detailed validation
    pub fn verify_quote_detailed(
        &self,
        quote_bytes: &[u8],
        expected_report_data: Option<&[u8; 64]>,
    ) -> TeeResult<QuoteValidationResult> {
        let quote = match Quote::from_bytes(quote_bytes) {
            Some(q) => q,
            None => {
                return Err(TeeError::InvalidAttestationReport(
                    "Failed to parse quote from bytes".to_string(),
                ));
            }
        };

        // Verify report data if provided
        if let Some(expected) = expected_report_data {
            if &quote.report_data != expected {
                warn!("Quote report data mismatch");
                return Ok(QuoteValidationResult::InvalidSignature);
            }
        }

        let options = self.config.to_quote_options();
        let result = quote.validate(&options);
        if result != QuoteValidationResult::Valid {
            return Ok(result);
        }

        #[cfg(feature = "sgx-hw")]
        {
            if let Err(err) = crate::sgx::verify_quote_signature(quote_bytes) {
                warn!("SGX DCAP quote verification failed: {}", err);
                return Ok(QuoteValidationResult::InvalidSignature);
            }
        }

        Ok(QuoteValidationResult::Valid)
    }

    /// Get the expected MRENCLAVE for this enclave version
    pub fn expected_mrenclave(&self) -> [u8; 32] {
        // On real hardware, this should be computed from the enclave binary (MRENCLAVE).
        // In simulation mode we use a deterministic value to keep tests reproducible.
        let mut hasher = Sha256::new();
        hasher.update(b"neo-tee-enclave-v1");
        let hash = hasher.finalize();
        let mut result = [0u8; 32];
        result.copy_from_slice(&hash);
        result
    }

    /// Compute MRENCLAVE from enclave binary
    ///
    /// In production, this would hash the actual enclave binary.
    /// For now, returns a deterministic value based on version.
    pub fn compute_mrenclave(&self, enclave_binary: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(b"neo-tee-mrenclave-v1");
        hasher.update(enclave_binary);
        let hash = hasher.finalize();
        let mut result = [0u8; 32];
        result.copy_from_slice(&hash);
        result
    }

    /// Verify MRENCLAVE against a known good value
    pub fn verify_mrenclave(
        &self,
        report: &AttestationReport,
        expected: &[u8; 32],
    ) -> TeeResult<()> {
        if !report.verify_mrenclave(expected) {
            Err(TeeError::mrenclave_mismatch(expected, &report.mrenclave))
        } else {
            Ok(())
        }
    }

    /// Verify MRSIGNER against a known good value
    pub fn verify_mrsigner(
        &self,
        report: &AttestationReport,
        expected: &[u8; 32],
    ) -> TeeResult<()> {
        if !report.verify_mrsigner(expected) {
            Err(TeeError::mrsigner_mismatch(expected, &report.mrsigner))
        } else {
            Ok(())
        }
    }

    /// Batch verify multiple attestation reports
    pub fn verify_reports_batch(
        &self,
        reports: &[AttestationReport],
    ) -> Vec<(usize, QuoteValidationResult)> {
        reports
            .iter()
            .enumerate()
            .map(|(idx, report)| {
                let result = self.verify_report_detailed(report);
                (idx, result)
            })
            .collect()
    }

    #[cfg(feature = "sgx-hw")]
    fn generate_sgx_report(&self, report_data: [u8; 64]) -> TeeResult<AttestationReport> {
        if self._enclave.config().simulation {
            if !self.config.allow_simulated {
                return Err(TeeError::FeatureNotEnabled(
                    "simulated attestation is disabled and enclave is in simulation mode"
                        .to_string(),
                ));
            }

            debug!(
                "SGX hardware feature enabled but enclave is in simulation mode; emitting simulated attestation report"
            );
            return Ok(AttestationReport::simulated_with_measurements(
                report_data,
                self.expected_mrenclave(),
                [0u8; 32],
            ));
        }

        let evidence = self._enclave.sgx_evidence().ok_or_else(|| {
            TeeError::FeatureNotEnabled(
                "strict SGX mode requires verified quote evidence during enclave initialization"
                    .to_string(),
            )
        })?;

        if evidence.report_data != report_data {
            warn!(
                "requested report_data differs from verified SGX quote report_data; returning quote-bound report_data in strict mode"
            );
        }

        Ok(AttestationReport {
            version: 1,
            report_type: ReportType::Remote,
            mrenclave: evidence.mrenclave,
            mrsigner: evidence.mrsigner,
            isv_prod_id: evidence.isv_prod_id,
            isv_svn: evidence.isv_svn,
            report_data: evidence.report_data,
            timestamp: std::time::SystemTime::now(),
            cpu_svn: evidence.cpu_svn,
            attributes: crate::attestation::report::EnclaveAttributes {
                debug: (evidence.attributes[0] & 0x02) != 0,
                mode64bit: (evidence.attributes[0] & 0x04) != 0,
                provision_key: (evidence.attributes[0] & 0x10) != 0,
                einit_token: (evidence.attributes[0] & 0x20) != 0,
                key_separation: true,
            },
            raw_report: evidence.quote.clone(),
            quote: Some(evidence.quote),
        })
    }

    #[cfg(not(feature = "sgx-hw"))]
    fn generate_simulated_report(&self, report_data: [u8; 64]) -> TeeResult<AttestationReport> {
        trace_info!("Generating simulated attestation report");
        Ok(AttestationReport::simulated(report_data))
    }
}

#[cfg(test)]
mod tests;
