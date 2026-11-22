use super::*;
impl AttestationService {
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
