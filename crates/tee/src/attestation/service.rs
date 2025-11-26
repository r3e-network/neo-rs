//! Attestation service for generating and verifying reports

use crate::enclave::TeeEnclave;
use crate::error::{TeeError, TeeResult};
use crate::attestation::{AttestationReport, report::ReportType};
use sha2::{Digest, Sha256};
use std::sync::Arc;
use tracing::{debug, info};

/// Service for generating and verifying attestation reports
pub struct AttestationService {
    enclave: Arc<TeeEnclave>,
}

impl AttestationService {
    /// Create a new attestation service
    pub fn new(enclave: Arc<TeeEnclave>) -> TeeResult<Self> {
        if !enclave.is_ready() {
            return Err(TeeError::EnclaveNotInitialized);
        }

        Ok(Self { enclave })
    }

    /// Generate an attestation report with custom report data
    pub fn generate_report(&self, user_data: &[u8]) -> TeeResult<AttestationReport> {
        // Create report data from user data (max 64 bytes)
        let mut report_data = [0u8; 64];
        let len = user_data.len().min(64);
        report_data[..len].copy_from_slice(&user_data[..len]);

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
    pub fn attest_ordering(&self, merkle_root: &[u8; 32], batch_id: u64) -> TeeResult<AttestationReport> {
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

    /// Verify an attestation report
    pub fn verify_report(&self, report: &AttestationReport) -> TeeResult<bool> {
        // Basic validation
        if report.version == 0 {
            return Ok(false);
        }

        // Check timestamp is reasonable (within 24 hours)
        let now = std::time::SystemTime::now();
        let age = now
            .duration_since(report.timestamp)
            .unwrap_or(std::time::Duration::from_secs(u64::MAX));

        if age.as_secs() > 24 * 60 * 60 {
            debug!("Report too old: {:?}", age);
            return Ok(false);
        }

        // Verify based on report type
        Ok(report.verify())
    }

    /// Verify that a report matches expected enclave measurements
    pub fn verify_enclave_identity(
        &self,
        report: &AttestationReport,
        expected_mrenclave: Option<&[u8; 32]>,
        expected_mrsigner: Option<&[u8; 32]>,
    ) -> bool {
        if let Some(expected) = expected_mrenclave {
            if &report.mrenclave != expected {
                debug!("MRENCLAVE mismatch");
                return false;
            }
        }

        if let Some(expected) = expected_mrsigner {
            if &report.mrsigner != expected {
                debug!("MRSIGNER mismatch");
                return false;
            }
        }

        true
    }

    /// Get the expected MRENCLAVE for this enclave version
    pub fn expected_mrenclave(&self) -> [u8; 32] {
        // In production, this would be computed from the enclave binary
        let mut hasher = Sha256::new();
        hasher.update(b"neo-tee-enclave-v1");
        let hash = hasher.finalize();
        let mut result = [0u8; 32];
        result.copy_from_slice(&hash);
        result
    }

    #[cfg(feature = "sgx-hw")]
    fn generate_sgx_report(&self, report_data: [u8; 64]) -> TeeResult<AttestationReport> {
        use sgx_isa::{Report, Targetinfo};

        // Get target info for self-report
        let target_info = Targetinfo::default();

        // Generate report
        let report = Report::for_target(&target_info, &report_data);

        // Extract fields from SGX report
        Ok(AttestationReport {
            version: 1,
            report_type: ReportType::Local,
            mrenclave: report.mrenclave,
            mrsigner: report.mrsigner,
            isv_prod_id: report.isvprodid,
            isv_svn: report.isvsvn,
            report_data,
            timestamp: std::time::SystemTime::now(),
            cpu_svn: report.cpusvn,
            attributes: super::report::EnclaveAttributes {
                debug: report.attributes.flags.contains(sgx_isa::AttributesFlags::DEBUG),
                mode64bit: report.attributes.flags.contains(sgx_isa::AttributesFlags::MODE64BIT),
                provision_key: report.attributes.flags.contains(sgx_isa::AttributesFlags::PROVISIONKEY),
                einit_token: report.attributes.flags.contains(sgx_isa::AttributesFlags::EINITTOKENKEY),
                key_separation: false,
            },
            raw_report: report.as_ref().to_vec(),
            quote: None,
        })
    }

    #[cfg(not(feature = "sgx-hw"))]
    fn generate_simulated_report(&self, report_data: [u8; 64]) -> TeeResult<AttestationReport> {
        info!("Generating simulated attestation report");
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

        let service = AttestationService::new(enclave).unwrap();
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
}
