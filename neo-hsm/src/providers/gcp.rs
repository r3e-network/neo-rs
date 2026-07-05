//! Native GCP Cloud KMS signer (feature `gcp`).
//!
//! This backend calls the GCP Cloud KMS `AsymmetricSign` API directly via the
//! Cloud KMS REST endpoint, without requiring `libkmsp11.so`.  It is the
//! preferred path for operators who want pure-Rust ADC / Workload Identity
//! credentials without any FFI dependency.
//!
//! GCP `AsymmetricSign` returns a **DER-encoded** signature, so the
//! `DerToRawRS` post-processing (same as the GCP PKCS#11 path) is applied.
//!
//! # Feature status
//!
//! The `gcp` feature compiles this module stub.  A full implementation requires
//! the `google-cloud-kms` crate (yoshidan, async) or the official
//! `google-cloud-kms-v1` crate — both are out of scope for this stub because
//! they pull in heavy async/tonic/prost dependency trees.  The stub compiles
//! cleanly but returns [`HsmError::GcpFeatureNotEnabled`] at runtime.
//!
//! Operators using GCP Cloud KMS should prefer the PKCS#11 path via
//! [`Pkcs11Signer`] with `provider = GcpCloudHsm` and `libkmsp11.so`, which
//! is fully implemented and requires no additional SDK crates.

use crate::error::{HsmError, HsmResult};
use async_trait::async_trait;
use neo_consensus::ConsensusSigner;
use neo_consensus::error::ConsensusError;
use neo_primitives::UInt160;

/// Configuration for the GCP Cloud KMS native REST signer.
#[derive(Debug, Clone)]
pub struct GcpKmsConfig {
    /// Full CryptoKeyVersion resource name.
    ///
    /// Format: `projects/P/locations/L/keyRings/R/cryptoKeys/K/cryptoKeyVersions/N`
    pub key_resource: String,

    /// Neo consensus script hash for this key (supplied by the operator after
    /// registering the key's compressed public key).
    pub script_hash: UInt160,
}

/// GCP Cloud KMS native `AsymmetricSign` signer.
///
/// Currently a stub — returns [`HsmError::GcpFeatureNotEnabled`] at runtime.
/// Use the PKCS#11 path (`feature = "pkcs11"`, `provider = GcpCloudHsm`) for
/// a fully operational GCP signer.
pub struct GcpKmsSigner {
    cfg: GcpKmsConfig,
}

impl GcpKmsSigner {
    /// Create a new signer (stub — does not connect to GCP).
    #[allow(clippy::missing_errors_doc)]
    pub fn new(cfg: GcpKmsConfig) -> HsmResult<Self> {
        Ok(Self { cfg })
    }
}

#[async_trait]
impl ConsensusSigner for GcpKmsSigner {
    fn can_sign(&self, script_hash: &UInt160) -> bool {
        *script_hash == self.cfg.script_hash
    }

    async fn sign(&self, _data: &[u8], script_hash: &UInt160) -> Result<Vec<u8>, ConsensusError> {
        if !self.can_sign(script_hash) {
            return Err(ConsensusError::state_error(format!(
                "hsm-gcp: unknown script hash {script_hash}"
            )));
        }
        // Stub: the full async GCP SDK is not wired.
        // Use the PKCS#11 path (feature="pkcs11", provider=GcpCloudHsm) for
        // a working GCP signer via libkmsp11.so.
        Err(HsmError::GcpFeatureNotEnabled.into())
    }
}
