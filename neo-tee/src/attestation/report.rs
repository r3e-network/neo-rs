//! Attestation report structures

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::time::{Duration, SystemTime};

/// Maximum age for a valid attestation report (24 hours)
pub const MAX_REPORT_AGE_SECONDS: u64 = 24 * 60 * 60;

/// Minimum security version number (SVN) allowed
pub const MIN_SECURITY_VERSION: u16 = 1;

/// Quote validation result
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QuoteValidationResult {
    /// Quote is valid and can be trusted
    Valid,
    /// Quote has expired (too old)
    Expired,
    /// Quote MRENCLAVE doesn't match expected
    InvalidMrEnclave,
    /// Quote MRSIGNER doesn't match expected
    InvalidMrSigner,
    /// Security version is too low
    SecurityVersionTooLow,
    /// Quote has been revoked
    Revoked,
    /// Quote signature is invalid
    InvalidSignature,
    /// Quote format is invalid
    InvalidFormat,
    /// Unknown or unsupported quote type
    UnsupportedQuoteType,
}

impl QuoteValidationResult {
    /// Check if the validation result indicates success
    pub fn is_valid(&self) -> bool {
        matches!(self, QuoteValidationResult::Valid)
    }

    /// Get a human-readable description of the result
    pub fn description(&self) -> &'static str {
        match self {
            QuoteValidationResult::Valid => "Quote is valid",
            QuoteValidationResult::Expired => "Quote has expired",
            QuoteValidationResult::InvalidMrEnclave => "MRENCLAVE mismatch",
            QuoteValidationResult::InvalidMrSigner => "MRSIGNER mismatch",
            QuoteValidationResult::SecurityVersionTooLow => "Security version too low",
            QuoteValidationResult::Revoked => "Quote or platform has been revoked",
            QuoteValidationResult::InvalidSignature => "Invalid quote signature",
            QuoteValidationResult::InvalidFormat => "Invalid quote format",
            QuoteValidationResult::UnsupportedQuoteType => "Unsupported quote type",
        }
    }
}

/// Helper module for serializing/deserializing [u8; 64]
mod bytes64 {
    use super::*;

    pub fn serialize<S>(data: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(data))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Bytes64Visitor;

        impl<'de> Visitor<'de> for Bytes64Visitor {
            type Value = [u8; 64];

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a hex string of 64 bytes")
            }

            fn visit_str<E>(self, v: &str) -> Result<[u8; 64], E>
            where
                E: de::Error,
            {
                let bytes = hex::decode(v).map_err(de::Error::custom)?;
                if bytes.len() != 64 {
                    return Err(de::Error::custom(format!(
                        "expected 64 bytes, got {}",
                        bytes.len()
                    )));
                }
                let mut arr = [0u8; 64];
                arr.copy_from_slice(&bytes);
                Ok(arr)
            }
        }

        deserializer.deserialize_str(Bytes64Visitor)
    }
}

/// Helper module for serializing/deserializing [u8; 16]
mod bytes16 {
    use super::*;

    pub fn serialize<S>(data: &[u8; 16], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(data))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 16], D::Error>
    where
        D: Deserializer<'de>,
    {
        struct Bytes16Visitor;

        impl<'de> Visitor<'de> for Bytes16Visitor {
            type Value = [u8; 16];

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a hex string of 16 bytes")
            }

            fn visit_str<E>(self, v: &str) -> Result<[u8; 16], E>
            where
                E: de::Error,
            {
                let bytes = hex::decode(v).map_err(de::Error::custom)?;
                if bytes.len() != 16 {
                    return Err(de::Error::custom(format!(
                        "expected 16 bytes, got {}",
                        bytes.len()
                    )));
                }
                let mut arr = [0u8; 16];
                arr.copy_from_slice(&bytes);
                Ok(arr)
            }
        }

        deserializer.deserialize_str(Bytes16Visitor)
    }
}

/// SGX attestation report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationReport {
    /// Report version
    pub version: u16,
    /// Report type (local or remote)
    pub report_type: ReportType,
    /// MRENCLAVE - hash of enclave code
    pub mrenclave: [u8; 32],
    /// MRSIGNER - hash of enclave signer
    pub mrsigner: [u8; 32],
    /// ISV Product ID
    pub isv_prod_id: u16,
    /// ISV Security Version Number
    pub isv_svn: u16,
    /// Report data (user-provided data bound to report)
    #[serde(with = "bytes64")]
    pub report_data: [u8; 64],
    /// Timestamp when report was generated
    pub timestamp: SystemTime,
    /// CPU SVN at report generation
    #[serde(with = "bytes16")]
    pub cpu_svn: [u8; 16],
    /// Attributes flags
    pub attributes: EnclaveAttributes,
    /// Raw report bytes (for verification)
    pub raw_report: Vec<u8>,
    /// Quote (for remote attestation)
    pub quote: Option<Vec<u8>>,
}

/// Type of attestation report
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ReportType {
    /// Local attestation (between enclaves on same platform)
    Local,
    /// Remote attestation (verifiable by remote party)
    Remote,
    /// Simulated report (for testing)
    Simulated,
}

/// Enclave attributes
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct EnclaveAttributes {
    /// Debug mode enabled
    pub debug: bool,
    /// 64-bit mode
    pub mode64bit: bool,
    /// Provision key access
    pub provision_key: bool,
    /// EINIT token key access
    pub einit_token: bool,
    /// Key separation enabled
    pub key_separation: bool,
}

impl Default for EnclaveAttributes {
    fn default() -> Self {
        Self {
            debug: false,
            mode64bit: true,
            provision_key: false,
            einit_token: false,
            key_separation: true,
        }
    }
}

/// Quote structure for remote attestation
#[derive(Debug, Clone)]
pub struct Quote {
    /// Quote version
    pub version: u16,
    /// Quote signature type
    pub signature_type: u16,
    /// EPID group ID
    pub epid_group_id: [u8; 4],
    /// QE SVN (Quoting Enclave Security Version Number)
    pub qe_svn: [u8; 2],
    /// PCESVN (Provisioning Certificate Enclave SVN)
    pub pce_svn: [u8; 2],
    /// Extended EPID group ID
    pub xeid: u32,
    /// Report timestamp
    pub timestamp: [u8; 8],
    /// Report data from the enclave
    pub report_data: [u8; 64],
    /// MRENCLAVE measurement
    pub mrenclave: [u8; 32],
    /// MRSIGNER measurement
    pub mrsigner: [u8; 32],
    /// Report attributes
    pub attributes: [u8; 16],
    /// ISV Product ID
    pub isv_prod_id: u16,
    /// ISV SVN
    pub isv_svn: u16,
    /// Report ID
    pub report_id: [u8; 32],
    /// Report ID MAC
    pub report_id_ma: [u8; 32],
    /// CPU SVN
    pub cpu_svn: [u8; 16],
    /// Misc select
    pub misc_select: [u8; 4],
    /// Raw quote bytes
    pub raw_bytes: Vec<u8>,
}

/// Quote validation options
#[derive(Debug, Clone)]
pub struct QuoteValidationOptions {
    /// Expected MRENCLAVE (if None, skips verification)
    pub expected_mrenclave: Option<[u8; 32]>,
    /// Expected MRSIGNER (if None, skips verification)
    pub expected_mrsigner: Option<[u8; 32]>,
    /// Minimum acceptable ISV SVN
    pub min_isv_svn: u16,
    /// Maximum age of the quote
    pub max_age: Duration,
    /// Require non-debug enclave
    pub require_non_debug: bool,
}

impl Default for QuoteValidationOptions {
    fn default() -> Self {
        Self {
            expected_mrenclave: None,
            expected_mrsigner: None,
            min_isv_svn: MIN_SECURITY_VERSION,
            max_age: Duration::from_secs(MAX_REPORT_AGE_SECONDS),
            require_non_debug: false,
        }
    }
}

impl AttestationReport {
    /// Create a simulated report for testing
    pub fn simulated(report_data: [u8; 64]) -> Self {
        use sha2::{Digest, Sha256};

        // Generate deterministic MRENCLAVE from code hash
        let mut hasher = Sha256::new();
        hasher.update(b"neo-tee-enclave-v1");
        let mrenclave_hash = hasher.finalize();
        let mut mrenclave = [0u8; 32];
        mrenclave.copy_from_slice(&mrenclave_hash);

        // Generate deterministic MRSIGNER
        let mut hasher = Sha256::new();
        hasher.update(b"neo-tee-signer-v1");
        let mrsigner_hash = hasher.finalize();
        let mut mrsigner = [0u8; 32];
        mrsigner.copy_from_slice(&mrsigner_hash);

        Self {
            version: 1,
            report_type: ReportType::Simulated,
            mrenclave,
            mrsigner,
            isv_prod_id: 1,
            isv_svn: 1,
            report_data,
            timestamp: SystemTime::now(),
            cpu_svn: [0u8; 16],
            attributes: EnclaveAttributes::default(),
            raw_report: Vec::new(),
            quote: None,
        }
    }

    /// Create a simulated report with specific measurements
    pub fn simulated_with_measurements(
        report_data: [u8; 64],
        mrenclave: [u8; 32],
        mrsigner: [u8; 32],
    ) -> Self {
        Self {
            version: 1,
            report_type: ReportType::Simulated,
            mrenclave,
            mrsigner,
            isv_prod_id: 1,
            isv_svn: 1,
            report_data,
            timestamp: SystemTime::now(),
            cpu_svn: [0u8; 16],
            attributes: EnclaveAttributes::default(),
            raw_report: Vec::new(),
            quote: None,
        }
    }

    /// Verify the report signature
    pub fn verify(&self) -> bool {
        match self.report_type {
            ReportType::Simulated => true, // Always valid in simulation
            ReportType::Local => {
                #[cfg(feature = "sgx-hw")]
                {
                    self.verify_local_report()
                }
                #[cfg(not(feature = "sgx-hw"))]
                {
                    false
                }
            }
            ReportType::Remote => {
                #[cfg(feature = "sgx-hw")]
                {
                    self.verify_remote_quote()
                }
                #[cfg(not(feature = "sgx-hw"))]
                {
                    false
                }
            }
        }
    }

    /// Verify the report is not expired
    pub fn verify_timestamp(&self, max_age: Duration) -> bool {
        let now = SystemTime::now();
        match now.duration_since(self.timestamp) {
            Ok(age) => age <= max_age,
            Err(_) => false, // Clock skew - timestamp in future
        }
    }

    /// Check if the security version is acceptable
    pub fn verify_security_version(&self, min_svn: u16) -> bool {
        self.isv_svn >= min_svn
    }

    /// Verify MRENCLAVE matches expected value
    pub fn verify_mrenclave(&self, expected: &[u8; 32]) -> bool {
        &self.mrenclave == expected
    }

    /// Verify MRSIGNER matches expected value
    pub fn verify_mrsigner(&self, expected: &[u8; 32]) -> bool {
        &self.mrsigner == expected
    }

    /// Verify that the enclave is not in debug mode
    pub fn verify_non_debug(&self) -> bool {
        !self.attributes.debug
    }

    /// Perform full report validation with options
    pub fn validate(&self, options: &QuoteValidationOptions) -> QuoteValidationResult {
        // Check timestamp
        if !self.verify_timestamp(options.max_age) {
            return QuoteValidationResult::Expired;
        }

        // Check security version
        if !self.verify_security_version(options.min_isv_svn) {
            return QuoteValidationResult::SecurityVersionTooLow;
        }

        // Check MRENCLAVE if specified
        if let Some(expected) = &options.expected_mrenclave {
            if !self.verify_mrenclave(expected) {
                return QuoteValidationResult::InvalidMrEnclave;
            }
        }

        // Check MRSIGNER if specified
        if let Some(expected) = &options.expected_mrsigner {
            if !self.verify_mrsigner(expected) {
                return QuoteValidationResult::InvalidMrSigner;
            }
        }

        // Check debug mode requirement
        if options.require_non_debug && !self.verify_non_debug() {
            return QuoteValidationResult::InvalidSignature;
        }

        // Verify signature/quote based on type
        if !self.verify() {
            return QuoteValidationResult::InvalidSignature;
        }

        QuoteValidationResult::Valid
    }

    #[cfg(feature = "sgx-hw")]
    fn verify_local_report(&self) -> bool {
        // Real SGX local report verification requires EREPORT/EREPORTKEY inside an enclave.
        // Until the verifier is implemented, fail closed for non-simulated reports.
        tracing::warn!(
            target: "neo",
            "SGX local report verification is unavailable (failing closed)"
        );
        false
    }

    #[cfg(feature = "sgx-hw")]
    fn verify_remote_quote(&self) -> bool {
        let Some(quote) = self.quote.as_deref() else {
            tracing::warn!(target: "neo", "remote attestation report missing quote bytes");
            return false;
        };

        match crate::sgx::verify_quote_signature(quote) {
            Ok(_) => true,
            Err(err) => {
                tracing::warn!(
                    target: "neo",
                    error = %err,
                    "SGX remote quote verification failed"
                );
                false
            }
        }
    }

    /// Serialize report to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Deserialize report from bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        serde_json::from_slice(data).ok()
    }

    /// Extract quote from report if available
    pub fn get_quote(&self) -> Option<&[u8]> {
        self.quote.as_deref()
    }
}

impl Quote {
    /// Parse a raw SGX quote from bytes
    ///
    /// Supports SGX quote formats (v3, ECDSA quotes)
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        // sgx_quote3_t fixed-size prefix:
        // 48-byte header + 384-byte report_body + 4-byte signature_data_len.
        if bytes.len() < 436 {
            return None;
        }

        // Parse SGX quote v3 header.
        let version = u16::from_le_bytes([bytes[0], bytes[1]]);
        let signature_type = u16::from_le_bytes([bytes[2], bytes[3]]); // att_key_type

        // Preserve header att_key_data_0 in legacy fields.
        let mut epid_group_id = [0u8; 4];
        epid_group_id.copy_from_slice(&bytes[4..8]);

        let mut qe_svn = [0u8; 2];
        qe_svn.copy_from_slice(&bytes[8..10]);

        let mut pce_svn = [0u8; 2];
        pce_svn.copy_from_slice(&bytes[10..12]);

        // Legacy field in this struct. In SGX quote v3 this corresponds to att_key_data_0.
        let xeid = u32::from_le_bytes([bytes[4], bytes[5], bytes[6], bytes[7]]);

        // SGX quote v3 does not include a timestamp in the quote header.
        let mut timestamp = [0u8; 8];
        timestamp.fill(0);

        // Parse sgx_report_body_t fields from quote body.
        let report_body_offset = 48;
        let cpu_svn_offset = report_body_offset;
        let misc_select_offset = report_body_offset + 16;
        let attributes_offset = report_body_offset + 48;
        let mrenclave_offset = report_body_offset + 64;
        let mrsigner_offset = report_body_offset + 128;
        let isv_prod_id_offset = report_body_offset + 256;
        let isv_svn_offset = report_body_offset + 258;
        let report_data_offset = report_body_offset + 320;

        // Parse CPU SVN.
        let mut cpu_svn = [0u8; 16];
        cpu_svn.copy_from_slice(&bytes[cpu_svn_offset..cpu_svn_offset + 16]);

        // Parse misc select.
        let mut misc_select = [0u8; 4];
        misc_select.copy_from_slice(&bytes[misc_select_offset..misc_select_offset + 4]);

        // Parse attributes.
        let mut attributes = [0u8; 16];
        attributes.copy_from_slice(&bytes[attributes_offset..attributes_offset + 16]);

        // Parse MRENCLAVE and MRSIGNER.
        let mut mrenclave = [0u8; 32];
        mrenclave.copy_from_slice(&bytes[mrenclave_offset..mrenclave_offset + 32]);

        let mut mrsigner = [0u8; 32];
        mrsigner.copy_from_slice(&bytes[mrsigner_offset..mrsigner_offset + 32]);

        // Parse ISV product/security version.
        let isv_prod_id =
            u16::from_le_bytes([bytes[isv_prod_id_offset], bytes[isv_prod_id_offset + 1]]);
        let isv_svn = u16::from_le_bytes([bytes[isv_svn_offset], bytes[isv_svn_offset + 1]]);

        // Parse report_data.
        let mut report_data = [0u8; 64];
        report_data.copy_from_slice(&bytes[report_data_offset..report_data_offset + 64]);

        // Legacy fields not present in SGX quote v3 body.
        let report_id = [0u8; 32];
        let report_id_ma = [0u8; 32];

        Some(Self {
            version,
            signature_type,
            epid_group_id,
            qe_svn,
            pce_svn,
            xeid,
            timestamp,
            report_data,
            mrenclave,
            mrsigner,
            attributes,
            isv_prod_id,
            isv_svn,
            report_id,
            report_id_ma,
            cpu_svn,
            misc_select,
            raw_bytes: bytes.to_vec(),
        })
    }

    /// Validate the quote against expected values
    pub fn validate(&self, options: &QuoteValidationOptions) -> QuoteValidationResult {
        // Check quote version (v3 or later for ECDSA)
        if self.version < 3 {
            return QuoteValidationResult::UnsupportedQuoteType;
        }

        // Check MRENCLAVE if specified
        if let Some(expected) = &options.expected_mrenclave {
            if &self.mrenclave != expected {
                return QuoteValidationResult::InvalidMrEnclave;
            }
        }

        // Check MRSIGNER if specified
        if let Some(expected) = &options.expected_mrsigner {
            if &self.mrsigner != expected {
                return QuoteValidationResult::InvalidMrSigner;
            }
        }

        // Check security version
        if self.isv_svn < options.min_isv_svn {
            return QuoteValidationResult::SecurityVersionTooLow;
        }

        // Check debug mode
        if options.require_non_debug {
            // Check debug bit in attributes (bit 1)
            let debug_bit = self.attributes[0] & 0x02;
            if debug_bit != 0 {
                return QuoteValidationResult::InvalidSignature;
            }
        }

        QuoteValidationResult::Valid
    }

    /// Get the report data as a hex string
    pub fn report_data_hex(&self) -> String {
        hex::encode(self.report_data)
    }

    /// Get the MRENCLAVE as a hex string
    pub fn mrenclave_hex(&self) -> String {
        hex::encode(self.mrenclave)
    }

    /// Get the MRSIGNER as a hex string
    pub fn mrsigner_hex(&self) -> String {
        hex::encode(self.mrsigner)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_simulated_report() {
        let report_data = [0x42u8; 64];
        let report = AttestationReport::simulated(report_data);

        assert_eq!(report.report_type, ReportType::Simulated);
        assert_eq!(report.report_data, report_data);
        assert!(report.verify());
    }

    #[test]
    fn test_report_serialization() {
        let report = AttestationReport::simulated([0u8; 64]);
        let bytes = report.to_bytes();
        let restored = AttestationReport::from_bytes(&bytes).unwrap();

        assert_eq!(restored.mrenclave, report.mrenclave);
        assert_eq!(restored.mrsigner, report.mrsigner);
    }

    #[test]
    fn test_timestamp_verification() {
        let report = AttestationReport::simulated([0u8; 64]);

        // Should be valid within 24 hours
        assert!(report.verify_timestamp(Duration::from_secs(MAX_REPORT_AGE_SECONDS)));

        // Should be valid within 1 second (assuming test runs fast)
        assert!(report.verify_timestamp(Duration::from_secs(1)));

        // Should be invalid for 0 seconds
        assert!(!report.verify_timestamp(Duration::from_secs(0)));
    }

    #[test]
    fn test_security_version_verification() {
        let mut report = AttestationReport::simulated([0u8; 64]);
        report.isv_svn = 5;

        assert!(report.verify_security_version(1));
        assert!(report.verify_security_version(5));
        assert!(!report.verify_security_version(6));
    }

    #[test]
    fn test_mrenclave_verification() {
        let expected = [0x42u8; 32];
        let report = AttestationReport::simulated_with_measurements([0u8; 64], expected, [0u8; 32]);

        assert!(report.verify_mrenclave(&expected));
        assert!(!report.verify_mrenclave(&[0x00u8; 32]));
    }

    #[test]
    fn test_mrsigner_verification() {
        let expected = [0x42u8; 32];
        let report = AttestationReport::simulated_with_measurements([0u8; 64], [0u8; 32], expected);

        assert!(report.verify_mrsigner(&expected));
        assert!(!report.verify_mrsigner(&[0x00u8; 32]));
    }

    #[test]
    fn test_full_validation() {
        let mrenclave = [0x42u8; 32];
        let mrsigner = [0x43u8; 32];

        let report = AttestationReport::simulated_with_measurements([0u8; 64], mrenclave, mrsigner);

        let options = QuoteValidationOptions {
            expected_mrenclave: Some(mrenclave),
            expected_mrsigner: Some(mrsigner),
            min_isv_svn: 1,
            max_age: Duration::from_secs(MAX_REPORT_AGE_SECONDS),
            require_non_debug: false,
        };

        assert_eq!(report.validate(&options), QuoteValidationResult::Valid);

        // Wrong MRENCLAVE
        let wrong_options = QuoteValidationOptions {
            expected_mrenclave: Some([0x00u8; 32]),
            expected_mrsigner: Some(mrsigner),
            min_isv_svn: 1,
            max_age: Duration::from_secs(MAX_REPORT_AGE_SECONDS),
            require_non_debug: false,
        };
        assert_eq!(
            report.validate(&wrong_options),
            QuoteValidationResult::InvalidMrEnclave
        );
    }

    #[test]
    fn test_quote_validation_result() {
        assert!(QuoteValidationResult::Valid.is_valid());
        assert!(!QuoteValidationResult::Expired.is_valid());
        assert!(!QuoteValidationResult::InvalidMrEnclave.is_valid());

        assert_eq!(QuoteValidationResult::Valid.description(), "Quote is valid");
        assert_eq!(
            QuoteValidationResult::Expired.description(),
            "Quote has expired"
        );
    }

    #[test]
    fn test_quote_parse_too_short() {
        let short_bytes = vec![0u8; 100];
        assert!(Quote::from_bytes(&short_bytes).is_none());
    }

    #[test]
    fn test_quote_parse_minimal() {
        // Create a minimal valid SGX quote v3 structure (436-byte fixed prefix).
        let mut bytes = vec![0u8; 436];

        // Set version to 3 (ECDSA)
        bytes[0] = 3;
        bytes[1] = 0;

        // Set signature type to ECDSA
        bytes[2] = 2;
        bytes[3] = 0;

        // Set ISV SVN to valid value (at offset 48 + 258)
        bytes[48 + 258] = 1;
        bytes[48 + 259] = 0;

        let quote = Quote::from_bytes(&bytes).unwrap();
        assert_eq!(quote.version, 3);
        assert_eq!(quote.signature_type, 2);
        assert_eq!(quote.isv_svn, 1);
    }

    #[test]
    fn test_quote_parse_uses_sgx_v3_report_body_offsets() {
        let mut bytes = vec![0u8; 436];
        bytes[0] = 3;
        bytes[2] = 2;

        let mrenclave = [0x11u8; 32];
        let mrsigner = [0x22u8; 32];
        let report_data = [0x33u8; 64];

        bytes[48 + 64..48 + 96].copy_from_slice(&mrenclave);
        bytes[48 + 128..48 + 160].copy_from_slice(&mrsigner);
        bytes[48 + 320..48 + 384].copy_from_slice(&report_data);

        bytes[48 + 48] = 0x02; // debug bit
        bytes[48 + 256] = 0x34;
        bytes[48 + 257] = 0x12; // isv_prod_id = 0x1234
        bytes[48 + 258] = 0x78;
        bytes[48 + 259] = 0x56; // isv_svn = 0x5678

        let quote = Quote::from_bytes(&bytes).expect("quote should parse");
        assert_eq!(quote.mrenclave, mrenclave);
        assert_eq!(quote.mrsigner, mrsigner);
        assert_eq!(quote.report_data, report_data);
        assert_eq!(quote.attributes[0] & 0x02, 0x02);
        assert_eq!(quote.isv_prod_id, 0x1234);
        assert_eq!(quote.isv_svn, 0x5678);
    }
}
