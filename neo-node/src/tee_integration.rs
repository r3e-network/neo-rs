//! TEE integration for neo-node
//!
//! This module provides TEE-protected wallet and mempool functionality.
//!
//! Note: This module is feature-gated and activated when the `tee` feature is enabled.

use neo_tee::{
    attestation::AttestationService,
    enclave::{EnclaveConfig, TeeEnclave},
    mempool::{FairOrderingPolicy, TeeMempool, TeeMempoolConfig},
    wallet::TeeWalletProvider,
};
use parking_lot::RwLock;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;
use tracing::{info, warn};

/// TEE runtime state
pub struct TeeRuntime {
    pub enclave: Arc<TeeEnclave>,
    pub wallet_provider: TeeWalletProvider,
    pub mempool: TeeMempool,
    pub attestation: AttestationService,
}

fn active_runtime_slot() -> &'static RwLock<Option<Weak<TeeRuntime>>> {
    static ACTIVE_RUNTIME: OnceLock<RwLock<Option<Weak<TeeRuntime>>>> = OnceLock::new();
    ACTIVE_RUNTIME.get_or_init(|| RwLock::new(None))
}

/// Registers the active TEE runtime for cross-module consumers (for example,
/// consensus transaction proposal ordering).
pub fn register_active_runtime(runtime: &Arc<TeeRuntime>) {
    *active_runtime_slot().write() = Some(Arc::downgrade(runtime));
}

/// Clears the active TEE runtime registration.
pub fn clear_active_runtime() {
    *active_runtime_slot().write() = None;
}

/// Returns the active TEE runtime if one is currently registered and alive.
pub fn active_runtime() -> Option<Arc<TeeRuntime>> {
    let mut guard = active_runtime_slot().write();
    if let Some(weak) = guard.as_ref() {
        if let Some(runtime) = weak.upgrade() {
            return Some(runtime);
        }
        *guard = None;
    }
    None
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
            allow_debug_in_production: cfg!(debug_assertions),
            ..Default::default()
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

    /// Runs startup self-checks to ensure core TEE logic is operational.
    ///
    /// This validates:
    /// - TEE mempool add/order/proof path
    /// - TEE wallet key creation + signature path
    pub fn run_startup_self_checks(&self) -> Result<(), neo_tee::TeeError> {
        let tx_hash: [u8; 32] = rand::random();
        let sender: [u8; 20] = rand::random();
        let payload = b"neo-node-tee-startup-self-check".to_vec();

        self.mempool
            .add_transaction(tx_hash, payload, 1_000, 0, sender)?;
        let ordered = self.mempool.get_ordered_hashes(1);
        if ordered.first().copied() != Some(tx_hash) {
            return Err(neo_tee::TeeError::OrderingError(
                "TEE mempool ordering self-check failed".to_string(),
            ));
        }

        let proof = self.mempool.generate_ordering_proof()?;
        if proof.public_key.is_empty() || proof.signature.is_empty() {
            return Err(neo_tee::TeeError::OrderingError(
                "TEE mempool ordering proof self-check failed".to_string(),
            ));
        }
        self.mempool.remove_transaction(&tx_hash);

        let wallet_path = self
            .enclave
            .config()
            .sealed_data_path
            .join("wallet-startup-self-check");
        let wallet = if TeeWalletProvider::is_tee_wallet(&wallet_path) {
            match self.wallet_provider.open_wallet(&wallet_path) {
                Ok(wallet) => wallet,
                Err(err) if is_stale_startup_wallet_error(&err) => {
                    warn!(
                        target: "neo::tee",
                        path = %wallet_path.display(),
                        error = %err,
                        "startup self-check wallet is stale for current sealing key; recreating it"
                    );
                    match std::fs::remove_dir_all(&wallet_path) {
                        Ok(()) => {}
                        Err(io_err) if io_err.kind() == ErrorKind::NotFound => {}
                        Err(io_err) => {
                            return Err(neo_tee::TeeError::Other(format!(
                                "failed to remove stale startup self-check wallet {}: {}",
                                wallet_path.display(),
                                io_err
                            )));
                        }
                    }
                    self.wallet_provider
                        .create_wallet("neo-node-tee-startup-self-check", &wallet_path)?
                }
                Err(err) => return Err(err),
            }
        } else {
            self.wallet_provider
                .create_wallet("neo-node-tee-startup-self-check", &wallet_path)?
        };

        match run_wallet_signing_self_check(&wallet) {
            Ok(()) => {}
            Err(err) if is_stale_startup_wallet_error(&err) => {
                warn!(
                    target: "neo::tee",
                    path = %wallet_path.display(),
                    error = %err,
                    "startup self-check wallet key material is stale; recreating and retrying"
                );
                match std::fs::remove_dir_all(&wallet_path) {
                    Ok(()) => {}
                    Err(io_err) if io_err.kind() == ErrorKind::NotFound => {}
                    Err(io_err) => {
                        return Err(neo_tee::TeeError::Other(format!(
                            "failed to remove stale startup self-check wallet {}: {}",
                            wallet_path.display(),
                            io_err
                        )));
                    }
                }
                let recreated_wallet = self
                    .wallet_provider
                    .create_wallet("neo-node-tee-startup-self-check", &wallet_path)?;
                run_wallet_signing_self_check(&recreated_wallet)?;
            }
            Err(err) => return Err(err),
        }

        Ok(())
    }
}

fn run_wallet_signing_self_check(wallet: &neo_tee::TeeWallet) -> Result<(), neo_tee::TeeError> {
    if wallet.list_keys().is_empty() {
        let _ = wallet.create_key(Some("startup-self-check".to_string()))?;
    }

    let default_key = wallet
        .default_account()
        .or_else(|| wallet.list_keys().into_iter().next())
        .ok_or_else(|| {
            neo_tee::TeeError::Other("TEE wallet self-check did not produce any keys".to_string())
        })?;
    let signature = wallet.sign(
        &default_key.script_hash,
        b"neo-node-tee-signature-self-check",
    )?;
    if signature.is_empty() {
        return Err(neo_tee::TeeError::Other(
            "TEE wallet self-check produced an empty signature".to_string(),
        ));
    }

    Ok(())
}

fn is_stale_startup_wallet_error(err: &neo_tee::TeeError) -> bool {
    matches!(
        err,
        neo_tee::TeeError::UnsealingFailed(_) | neo_tee::TeeError::CryptoError(_)
    )
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
        let runtime = TeeRuntime::new(temp.path().to_path_buf(), "batched", 1000);

        #[cfg(feature = "tee-sgx")]
        {
            match runtime {
                Err(err) => {
                    assert!(
                        matches!(err, neo_tee::TeeError::EnclaveInitError { .. }),
                        "unexpected error: {err}"
                    );
                }
                Ok(_) => {
                    panic!("tee-sgx should fail closed without verified SGX evidence");
                }
            }
        }

        #[cfg(not(feature = "tee-sgx"))]
        {
            let runtime = runtime.expect("TEE runtime should initialize in simulation mode");
            assert!(runtime.enclave.is_ready());
        }
    }

    #[test]
    fn test_tee_runtime_startup_self_checks() {
        let temp = tempdir().unwrap();
        let runtime = TeeRuntime::new(temp.path().to_path_buf(), "batched", 1000);

        #[cfg(feature = "tee-sgx")]
        {
            match runtime {
                Err(err) => {
                    assert!(
                        matches!(err, neo_tee::TeeError::EnclaveInitError { .. }),
                        "unexpected error: {err}"
                    );
                }
                Ok(_) => {
                    panic!("tee-sgx should fail closed without verified SGX evidence");
                }
            }
        }

        #[cfg(not(feature = "tee-sgx"))]
        {
            let runtime = runtime.expect("TEE runtime should initialize in simulation mode");
            runtime
                .run_startup_self_checks()
                .expect("TEE startup self-checks should pass");
        }
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

    #[cfg(feature = "tee-sgx")]
    #[test]
    fn test_tee_runtime_creation_with_real_sgx_evidence_when_opted_in() {
        if std::env::var("NEO_TEE_RUN_REAL_SGX_TEST").as_deref() != Ok("1") {
            return;
        }

        let temp = tempdir().unwrap();
        let runtime = TeeRuntime::new(temp.path().to_path_buf(), "batched", 1000)
            .expect("real SGX mode requires valid quote + sealing evidence");
        runtime
            .run_startup_self_checks()
            .expect("TEE startup self-checks should pass with real SGX evidence");
    }
}
