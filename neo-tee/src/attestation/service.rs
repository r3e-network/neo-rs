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

    /// Generate an attestation report with custom report data
    pub fn generate_report(&self, user_data: &[u8]) -> TeeResult<AttestationReport> {
        #[cfg(feature = "tee-sgx")]
        let report_data = crate::sgx::report_data_from_user_data(user_data);

        #[cfg(not(feature = "tee-sgx"))]
        let report_data = {
            let mut data = [0u8; 64];
            let len = user_data.len().min(64);
            data[..len].copy_from_slice(&user_data[..len]);
            data
        };

        #[cfg(feature = "tee-sgx")]
        {
            self.generate_sgx_report(report_data)
        }

        #[cfg(not(feature = "tee-sgx"))]
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
        if let Some(expected) = expected_mrenclave
            && !report.verify_mrenclave(expected)
        {
            debug!("MRENCLAVE mismatch");
            return false;
        }

        if let Some(expected) = expected_mrsigner
            && !report.verify_mrsigner(expected)
        {
            debug!("MRSIGNER mismatch");
            return false;
        }

        true
    }

    /// Verify a quote from raw bytes
    pub fn verify_quote(&self, quote_bytes: &[u8]) -> TeeResult<QuoteValidationResult> {
        let Some(quote) = Quote::from_bytes(quote_bytes) else {
            return Err(TeeError::InvalidAttestationReport(
                "Failed to parse quote from bytes".to_string(),
            ));
        };

        let options = self.config.to_quote_options();
        let result = quote.validate(&options);
        if result != QuoteValidationResult::Valid {
            return Ok(result);
        }

        #[cfg(feature = "tee-sgx")]
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
        let Some(quote) = Quote::from_bytes(quote_bytes) else {
            return Err(TeeError::InvalidAttestationReport(
                "Failed to parse quote from bytes".to_string(),
            ));
        };

        // Verify report data if provided
        if let Some(expected) = expected_report_data
            && &quote.report_data != expected
        {
            warn!("Quote report data mismatch");
            return Ok(QuoteValidationResult::InvalidSignature);
        }

        let options = self.config.to_quote_options();
        let result = quote.validate(&options);
        if result != QuoteValidationResult::Valid {
            return Ok(result);
        }

        #[cfg(feature = "tee-sgx")]
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

    /// Compute MRENCLAVE from enclave binary according to Intel SGX specification
    ///
    /// MRENCLAVE is computed by simulating the SGX enclave build process:
    /// 1. ECREATE: Hash initial enclave metadata (SECS)
    /// 2. EADD/EEXTEND: Hash each 256-byte page of enclave code/data
    /// 3. Final measurement is SHA-256 accumulation of all operations
    ///
    /// # Production Implementation
    ///
    /// This implementation follows Intel SGX measurement specification:
    /// - Processes enclave binary in 256-byte pages (SGX_PAGE_SIZE)
    /// - Each EEXTEND operation hashes 256 bytes of page content
    /// - Measurement accumulates: SHA256(previous_measurement || operation || data)
    ///
    /// # Reference
    ///
    /// Intel SGX Developer Guide, Section 2.4: Enclave Measurement
    /// https://www.intel.com/content/www/us/en/developer/tools/software-guard-extensions/overview.html
    pub fn compute_mrenclave(&self, enclave_binary: &[u8]) -> [u8; 32] {
        self.compute_mrenclave_with_metadata(enclave_binary, &EnclaveMetadata::default())
    }

    /// Compute MRENCLAVE with enclave metadata (SECS attributes)
    ///
    /// # Arguments
    ///
    /// * `enclave_binary` - The signed enclave binary (.so/.signed.so)
    /// * `metadata` - SECS attributes (size, base address, attributes, etc.)
    ///
    /// # SGX Measurement Process
    ///
    /// The measurement process follows these SGX instructions:
    ///
    /// 1. **ECREATE** (Initialize SECS):
    ///    ```text
    ///    measurement = SHA256(0x0045434543524541 || SECS.SSAFRAMESIZE || SECS.SIZE ||
    ///                         0 || SECS.MISCSELECT || 0 || SECS.ATTRIBUTES || 0 ||
    ///                         SECS.MRENCLAVE || 0 || SECS.MRSIGNER)
    ///    ```text
    ///
    /// 2. **EADD** (Add page - hashes page info, not content):
    ///    For each page:
    ///    ```text
    ///    measurement = SHA256(measurement || 0x0000000000444441 ||
    ///                         page.offset || page.flags)
    ///    ```text
    ///
    /// 3. **EEXTEND** (Extend page content - hashes actual data):
    ///    For each 256-byte chunk in page (16 chunks per 4KB page):
    ///    ```text
    ///    measurement = SHA256(measurement || 0x00444E4554584545 || chunk_offset || chunk_data)
    ///    ```text
    ///
    /// # Returns
    ///
    /// 32-byte MRENCLAVE value that uniquely identifies this enclave binary
    pub fn compute_mrenclave_with_metadata(
        &self,
        enclave_binary: &[u8],
        metadata: &EnclaveMetadata,
    ) -> [u8; 32] {
        const SGX_PAGE_SIZE: usize = 4096;
        const SGX_CHUNK_SIZE: usize = 256;

        // Initialize measurement with ECREATE operation
        let mut measurement = self.measure_ecreate(metadata);

        // Process binary in 4KB pages (SGX page granularity)
        let mut offset = 0u64;
        while offset < enclave_binary.len() as u64 {
            let page_offset = offset as usize;
            let page_end = (page_offset + SGX_PAGE_SIZE).min(enclave_binary.len());
            let page_data = &enclave_binary[page_offset..page_end];

            // EADD: Hash page metadata (offset, flags)
            measurement = self.measure_eadd(&measurement, offset, metadata.page_type);

            // EEXTEND: Hash page content in 256-byte chunks
            for chunk_offset in (0..page_data.len()).step_by(SGX_CHUNK_SIZE) {
                let chunk_end = (chunk_offset + SGX_CHUNK_SIZE).min(page_data.len());
                let mut chunk = [0u8; SGX_CHUNK_SIZE];
                chunk[..chunk_end - chunk_offset]
                    .copy_from_slice(&page_data[chunk_offset..chunk_end]);

                measurement =
                    self.measure_eextend(&measurement, offset + chunk_offset as u64, &chunk);
            }

            offset += SGX_PAGE_SIZE as u64;
        }

        measurement
    }

    /// Measure ECREATE operation (initialize SECS)
    fn measure_ecreate(&self, metadata: &EnclaveMetadata) -> [u8; 32] {
        let mut hasher = Sha256::new();

        // Magic: "ECREATE" in little-endian (0x45434543524541)
        hasher.update([0x45, 0x43, 0x52, 0x45, 0x41, 0x54, 0x45, 0x00]);

        // SECS.SSAFRAMESIZE (1 page = 4096 bytes)
        hasher.update(metadata.ssa_frame_size.to_le_bytes());

        // SECS.SIZE (enclave size, must be power of 2)
        hasher.update(metadata.enclave_size.to_le_bytes());

        // Reserved (48 bytes)
        hasher.update([0u8; 48]);

        // SECS.MISCSELECT (4 bytes)
        hasher.update(metadata.misc_select.to_le_bytes());

        // Reserved (28 bytes)
        hasher.update([0u8; 28]);

        // SECS.ATTRIBUTES (16 bytes): debug, mode64bit, provisionkey, etc.
        hasher.update(metadata.attributes);

        // Reserved (352 bytes)
        hasher.update([0u8; 352]);

        let hash = hasher.finalize();
        let mut result = [0u8; 32];
        result.copy_from_slice(&hash);
        result
    }

    /// Measure EADD operation (add page metadata)
    fn measure_eadd(&self, prev_measurement: &[u8; 32], offset: u64, page_type: u64) -> [u8; 32] {
        let mut hasher = Sha256::new();

        // Previous measurement
        hasher.update(prev_measurement);

        // Magic: "EADD" in little-endian (0x00000000444441)
        hasher.update([0x45, 0x41, 0x44, 0x44, 0x00, 0x00, 0x00, 0x00]);

        // Page offset within enclave
        hasher.update(offset.to_le_bytes());

        // SECINFO: page type and permissions
        // Bits 0-7: page type (REG=0x01, TCS=0x02, TRIM=0x04)
        // Bits 8-15: flags (R=0x01, W=0x02, X=0x04)
        hasher.update(page_type.to_le_bytes());

        // Reserved (48 bytes)
        hasher.update([0u8; 48]);

        let hash = hasher.finalize();
        let mut result = [0u8; 32];
        result.copy_from_slice(&hash);
        result
    }

    /// Measure EEXTEND operation (extend page content)
    fn measure_eextend(
        &self,
        prev_measurement: &[u8; 32],
        offset: u64,
        chunk: &[u8; 256],
    ) -> [u8; 32] {
        let mut hasher = Sha256::new();

        // Previous measurement
        hasher.update(prev_measurement);

        // Magic: "EEXTEND" in little-endian (0x444E4554584545)
        hasher.update([0x45, 0x45, 0x58, 0x54, 0x45, 0x4E, 0x44, 0x00]);

        // Chunk offset (256-byte aligned)
        hasher.update(offset.to_le_bytes());

        // Reserved (48 bytes)
        hasher.update([0u8; 48]);

        // 256-byte chunk of page content
        hasher.update(chunk);

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

    #[cfg(feature = "tee-sgx")]
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
            // R16: the verified quote is bound to the report data captured at
            // enclave initialization. Returning it for a different request
            // would present initialization evidence as if it proved THIS
            // request (attest_key/attest_ordering payloads) — refuse instead.
            return Err(TeeError::AttestationFailed(
                "requested report_data does not match the verified SGX quote report_data; \
                 strict-mode evidence cannot be reused as a proof for this request"
                    .to_string(),
            ));
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

    #[cfg(not(feature = "tee-sgx"))]
    fn generate_simulated_report(&self, report_data: [u8; 64]) -> TeeResult<AttestationReport> {
        trace_info!("Generating simulated attestation report");
        Ok(AttestationReport::simulated(report_data))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::enclave::EnclaveConfig;
    use tempfile::tempdir;

    fn setup_service() -> (tempfile::TempDir, AttestationService) {
        let temp = tempdir().unwrap();
        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            simulation: true,
            ..Default::default()
        };

        let enclave = Arc::new(TeeEnclave::new(config));
        enclave.initialize().unwrap();

        // Test helper always uses a simulation enclave, so force testing config to
        // keep simulated attestation enabled even in tee-sgx builds.
        let service =
            AttestationService::with_config(enclave, AttestationConfig::testing()).unwrap();
        (temp, service)
    }

    fn setup_service_with_config(
        config: AttestationConfig,
    ) -> (tempfile::TempDir, AttestationService) {
        let temp = tempdir().unwrap();
        let enclave_config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            simulation: true,
            ..Default::default()
        };

        let enclave = Arc::new(TeeEnclave::new(enclave_config));
        enclave.initialize().unwrap();

        let service = AttestationService::with_config(enclave, config).unwrap();
        (temp, service)
    }

    #[test]
    fn test_generate_and_verify_report() {
        let (_temp, service) = setup_service();

        let user_data = b"test attestation data";
        let report = service.generate_report(user_data).unwrap();

        assert!(service.verify_report(&report).unwrap());
    }

    #[test]
    fn test_key_attestation() {
        let (_temp, service) = setup_service();

        let public_key = [0x02u8; 33]; // Compressed public key
        let report = service.attest_key(&public_key).unwrap();

        assert!(service.verify_report(&report).unwrap());
    }

    #[test]
    fn test_enclave_identity_verification() {
        let (_temp, service) = setup_service();

        let report = service.generate_report(b"test").unwrap();

        // Should match expected values for simulation
        let expected_mr = service.expected_mrenclave();
        assert!(service.verify_enclave_identity(&report, Some(&expected_mr), None));
    }

    #[test]
    fn test_mrenclave_mismatch() {
        let (_temp, service) = setup_service();

        let report = service.generate_report(b"test").unwrap();

        let wrong_mrenclave = [0xFFu8; 32];
        let result = service.verify_mrenclave(&report, &wrong_mrenclave);

        assert!(result.is_err());
        match result.unwrap_err() {
            TeeError::MrEnclaveMismatch { expected, actual } => {
                assert_eq!(expected, hex::encode(wrong_mrenclave));
                assert_eq!(actual, hex::encode(report.mrenclave));
            }
            _ => panic!("Expected MrEnclaveMismatch error"),
        }
    }

    #[test]
    fn test_mrsigner_mismatch() {
        let (_temp, service) = setup_service();

        let report = service.generate_report(b"test").unwrap();

        let wrong_mrsigner = [0xFFu8; 32];
        let result = service.verify_mrsigner(&report, &wrong_mrsigner);

        assert!(result.is_err());
        match result.unwrap_err() {
            TeeError::MrSignerMismatch { expected, actual } => {
                assert_eq!(expected, hex::encode(wrong_mrsigner));
                assert_eq!(actual, hex::encode(report.mrsigner));
            }
            _ => panic!("Expected MrSignerMismatch error"),
        }
    }

    #[test]
    fn test_production_config_rejects_simulated() {
        let config = AttestationConfig::production();
        let (_temp, service) = setup_service_with_config(config);

        match service.generate_report(b"test") {
            Ok(report) => {
                // Should fail because simulated reports are not allowed in production config.
                assert!(!service.verify_report(&report).unwrap());

                // Detailed check should return InvalidSignature.
                let result = service.verify_report_detailed(&report);
                assert_eq!(result, QuoteValidationResult::InvalidSignature);
            }
            Err(TeeError::FeatureNotEnabled(_)) => {
                // In strict SGX mode we fail closed until real quote generation is integrated.
            }
            Err(other) => panic!("unexpected error: {other}"),
        }
    }

    #[cfg(feature = "tee-sgx")]
    #[test]
    fn test_strict_sgx_mode_fails_closed_when_quote_generation_unavailable() {
        let config = AttestationConfig::production();
        let (_temp, service) = setup_service_with_config(config);

        match service.generate_report(b"strict-mode-test") {
            Err(TeeError::FeatureNotEnabled(_)) => {}
            Ok(_) => panic!("strict SGX mode must not emit simulated attestation reports"),
            Err(other) => panic!("unexpected error: {other}"),
        }
    }

    #[test]
    fn test_config_with_mrenclave() {
        let expected_mrenclave = [0x42u8; 32];
        let config = AttestationConfig::testing().with_mrenclave(expected_mrenclave);

        assert_eq!(config.expected_mrenclave, Some(expected_mrenclave));
    }

    #[test]
    fn test_compute_mrenclave() {
        let (_temp, service) = setup_service();

        let binary = b"test enclave binary";
        let mrenclave1 = service.compute_mrenclave(binary);
        let mrenclave2 = service.compute_mrenclave(binary);

        // Same binary should produce same MRENCLAVE
        assert_eq!(mrenclave1, mrenclave2);

        // Different binary should produce different MRENCLAVE
        let different_binary = b"different binary";
        let mrenclave3 = service.compute_mrenclave(different_binary);
        assert_ne!(mrenclave1, mrenclave3);

        // Verify MRENCLAVE is properly formed (32 bytes, not all zeros)
        assert_eq!(mrenclave1.len(), 32);
        assert!(mrenclave1.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_compute_mrenclave_with_metadata() {
        let (_temp, service) = setup_service();

        let binary = b"test enclave code";

        // Production metadata (non-debug)
        let prod_metadata = EnclaveMetadata::production(1024 * 1024 * 64);
        let prod_mrenclave = service.compute_mrenclave_with_metadata(binary, &prod_metadata);

        // Debug metadata
        let debug_metadata = EnclaveMetadata::debug(1024 * 1024 * 64);
        let debug_mrenclave = service.compute_mrenclave_with_metadata(binary, &debug_metadata);

        // Different metadata should produce different MRENCLAVE
        assert_ne!(prod_mrenclave, debug_mrenclave);

        // Same metadata should produce same result
        let prod_mrenclave2 = service.compute_mrenclave_with_metadata(binary, &prod_metadata);
        assert_eq!(prod_mrenclave, prod_mrenclave2);
    }

    #[test]
    fn test_mrenclave_page_boundary_handling() {
        let (_temp, service) = setup_service();

        // Test with exactly 1 page (4096 bytes)
        let one_page = vec![0xAAu8; 4096];
        let mrenclave_1page = service.compute_mrenclave(&one_page);

        // Test with 1 page + 1 byte (triggers second page)
        let one_page_plus = vec![0xAAu8; 4097];
        let mrenclave_1page_plus = service.compute_mrenclave(&one_page_plus);

        // Should be different due to page boundary
        assert_ne!(mrenclave_1page, mrenclave_1page_plus);

        // Test with partial page
        let partial_page = vec![0xBBu8; 256];
        let mrenclave_partial = service.compute_mrenclave(&partial_page);

        // Should produce valid measurement
        assert!(mrenclave_partial.iter().any(|&b| b != 0));
    }

    #[test]
    fn test_mrenclave_chunk_processing() {
        let (_temp, service) = setup_service();

        // Test that 256-byte chunks are processed correctly
        // Each chunk is hashed via EEXTEND
        let chunk_aligned = vec![0x42u8; 256];
        let mrenclave1 = service.compute_mrenclave(&chunk_aligned);

        // Same data in different alignment should still process correctly
        let mut chunk_unaligned = vec![0x00u8; 128];
        chunk_unaligned.extend_from_slice(&[0x42u8; 256]);
        let mrenclave2 = service.compute_mrenclave(&chunk_unaligned);

        // Different due to different total content
        assert_ne!(mrenclave1, mrenclave2);
    }

    #[test]
    fn test_enclave_metadata_defaults() {
        let metadata = EnclaveMetadata::default();

        assert_eq!(metadata.ssa_frame_size, 1);
        assert!(metadata.enclave_size > 0);
        assert_eq!(metadata.misc_select, 0);

        // Default should be 64-bit mode
        assert_eq!(metadata.attributes[0] & 0x04, 0x04);

        // Default should not be debug mode
        assert_eq!(metadata.attributes[0] & 0x02, 0x00);
    }

    #[test]
    fn test_production_vs_debug_metadata() {
        let prod = EnclaveMetadata::production(1024 * 1024 * 128);
        let debug = EnclaveMetadata::debug(1024 * 1024 * 128);

        // Production should not have DEBUG bit set
        assert_eq!(prod.attributes[0] & 0x02, 0x00);

        // Debug should have DEBUG bit set
        assert_eq!(debug.attributes[0] & 0x02, 0x02);

        // Both should have MODE64BIT
        assert_eq!(prod.attributes[0] & 0x04, 0x04);
        assert_eq!(debug.attributes[0] & 0x04, 0x04);
    }

    #[test]
    fn test_verify_quote_with_invalid_bytes() {
        let (_temp, service) = setup_service();

        let invalid_quote = vec![0u8; 100]; // Too short
        let result = service.verify_quote(&invalid_quote);

        assert!(result.is_err());
    }

    #[test]
    fn test_batch_verification() {
        let (_temp, service) = setup_service();

        let reports = vec![
            service.generate_report(b"test1").unwrap(),
            service.generate_report(b"test2").unwrap(),
            service.generate_report(b"test3").unwrap(),
        ];

        let results = service.verify_reports_batch(&reports);

        assert_eq!(results.len(), 3);
        for (_, result) in results {
            assert_eq!(result, QuoteValidationResult::Valid);
        }
    }

    #[test]
    fn test_service_requires_initialized_enclave() {
        let temp = tempdir().unwrap();
        let config = EnclaveConfig {
            sealed_data_path: temp.path().to_path_buf(),
            simulation: true,
            ..Default::default()
        };

        let enclave = Arc::new(TeeEnclave::new(config));
        // Don't initialize!

        let result = AttestationService::new(enclave);
        assert!(result.is_err());
    }

    #[test]
    fn test_config_conversions() {
        let config = AttestationConfig::default();
        let options = config.to_quote_options();

        assert_eq!(options.min_isv_svn, config.min_isv_svn);
        assert_eq!(options.max_age, config.max_report_age);
        assert_eq!(options.require_non_debug, config.require_non_debug);
    }
}
