use super::*;
impl AttestationService {
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
                warn!("SGX DCAP quote verification failed: {err}");
                return Ok(QuoteValidationResult::InvalidSignature);
            }

            Ok(QuoteValidationResult::Valid)
        }

        // C-2: without `tee-sgx` only the structural checks above ran, so a
        // "Valid" result would assert authenticity that was never verified.
        #[cfg(not(feature = "tee-sgx"))]
        {
            warn!("SGX quote verification unavailable: neo-tee was built without `tee-sgx`");
            Err(hardware_verification_unavailable())
        }
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
                warn!("SGX DCAP quote verification failed: {err}");
                return Ok(QuoteValidationResult::InvalidSignature);
            }

            Ok(QuoteValidationResult::Valid)
        }

        // C-2: see `verify_quote` — structural checks alone must never be
        // reported as a verified quote.
        #[cfg(not(feature = "tee-sgx"))]
        {
            warn!("SGX quote verification unavailable: neo-tee was built without `tee-sgx`");
            Err(hardware_verification_unavailable())
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
}
