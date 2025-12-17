//! Attestation report structures

use serde::de::{self, Visitor};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;
use std::time::SystemTime;

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
                #[cfg(feature = "attestation")]
                {
                    self.verify_remote_quote()
                }
                #[cfg(not(feature = "attestation"))]
                {
                    false
                }
            }
        }
    }

    #[cfg(feature = "sgx-hw")]
    fn verify_local_report(&self) -> bool {
        // Real SGX local report verification requires EREPORT/EREPORTKEY inside an enclave.
        // Until the verifier is implemented, fail closed for non-simulated reports.
        tracing::warn!(target: "neo", "SGX local report verification not implemented");
        false
    }

    #[cfg(feature = "attestation")]
    fn verify_remote_quote(&self) -> bool {
        // Real remote quote verification requires IAS/DCAP integration.
        // Until the verifier is implemented, fail closed for non-simulated reports.
        tracing::warn!(target: "neo", "SGX remote quote verification not implemented");
        false
    }

    /// Serialize report to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    /// Deserialize report from bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        serde_json::from_slice(data).ok()
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
}
