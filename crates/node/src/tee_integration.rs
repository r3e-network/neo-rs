//! TEE integration for neo-node
//!
//! This module provides TEE-protected wallet and mempool functionality.

use neo_tee::{
    attestation::AttestationService,
    enclave::{EnclaveConfig, TeeEnclave},
    mempool::{FairOrderingPolicy, TeeMempool, TeeMempoolConfig},
    wallet::TeeWalletProvider,
};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tracing::{info, warn};

/// TEE runtime state
pub struct TeeRuntime {
    pub enclave: Arc<TeeEnclave>,
    pub wallet_provider: TeeWalletProvider,
    pub mempool: TeeMempool,
    pub attestation: AttestationService,
}

impl TeeRuntime {
    /// Initialize the TEE runtime
    pub fn new(
        data_path: PathBuf,
        ordering_policy: &str,
        mempool_capacity: usize,
    ) -> Result<Self, neo_tee::TeeError> {
        info!(target: "neo::tee", "Initializing TEE runtime");

        // Create enclave configuration
        let enclave_config = EnclaveConfig {
            sealed_data_path: data_path.clone(),
            debug_mode: cfg!(debug_assertions),
            heap_size_mb: 256,
            tcs_count: 4,
            simulation: !cfg!(feature = "tee-sgx"),
        };

        // Initialize enclave
        let enclave = Arc::new(TeeEnclave::new(enclave_config));
        enclave.initialize()?;

        // Log enclave mode
        if cfg!(feature = "tee-sgx") {
            info!(target: "neo::tee", "TEE enclave initialized in SGX hardware mode");
        } else {
            warn!(target: "neo::tee", "TEE enclave initialized in SIMULATION mode (not secure for production)");
        }

        // Parse ordering policy
        let policy = parse_ordering_policy(ordering_policy);
        info!(target: "neo::tee", policy = ?policy, "Using fair ordering policy");

        // Create TEE mempool
        let mempool_config = TeeMempoolConfig {
            capacity: mempool_capacity,
            ordering_policy: policy,
            batch_interval: Duration::from_millis(100),
            encrypt_contents: false,
        };
        let mempool = TeeMempool::new(enclave.clone(), mempool_config)?;

        // Create wallet provider
        let wallet_provider = TeeWalletProvider::new(enclave.clone())?;

        // Create attestation service
        let attestation = AttestationService::new(enclave.clone())?;

        info!(target: "neo::tee", "TEE runtime initialized successfully");

        Ok(Self {
            enclave,
            wallet_provider,
            mempool,
            attestation,
        })
    }

    /// Generate attestation report for the TEE
    pub fn generate_attestation(&self) -> Result<Vec<u8>, neo_tee::TeeError> {
        let report = self.attestation.generate_report(b"neo-node-attestation")?;
        Ok(report.to_bytes())
    }

    /// Shutdown the TEE runtime
    pub fn shutdown(&self) -> Result<(), neo_tee::TeeError> {
        info!(target: "neo::tee", "Shutting down TEE runtime");
        self.enclave.shutdown()
    }
}

fn parse_ordering_policy(policy: &str) -> FairOrderingPolicy {
    match policy.to_lowercase().as_str() {
        "fcfs" | "first-come-first-served" => FairOrderingPolicy::FirstComeFirstServed,
        "batched" | "batched-random" => FairOrderingPolicy::BatchedRandom {
            batch_interval_ms: 100,
        },
        "commit-reveal" => FairOrderingPolicy::CommitReveal {
            commit_duration_ms: 500,
            reveal_duration_ms: 500,
        },
        "threshold" | "threshold-encryption" => FairOrderingPolicy::ThresholdEncryption,
        "gas-cap" | "fcfs-gas-cap" => FairOrderingPolicy::FcfsWithGasCap {
            max_gas_multiplier: 2,
        },
        _ => {
            warn!(target: "neo::tee", policy = policy, "Unknown ordering policy, using default (batched-random)");
            FairOrderingPolicy::default()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_tee_runtime_creation() {
        let temp = tempdir().unwrap();
        let runtime = TeeRuntime::new(temp.path().to_path_buf(), "batched", 1000).unwrap();

        assert!(runtime.enclave.is_ready());
    }

    #[test]
    fn test_ordering_policy_parsing() {
        assert!(matches!(
            parse_ordering_policy("fcfs"),
            FairOrderingPolicy::FirstComeFirstServed
        ));
        assert!(matches!(
            parse_ordering_policy("batched"),
            FairOrderingPolicy::BatchedRandom { .. }
        ));
        assert!(matches!(
            parse_ordering_policy("commit-reveal"),
            FairOrderingPolicy::CommitReveal { .. }
        ));
    }
}
