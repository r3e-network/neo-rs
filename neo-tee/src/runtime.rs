//! TEE runtime assembly (privacy-mode encryption + enclave/mempool wiring).
//!
//! Phase 3 S6c: sank from neo-node's `tee_integration` module. The RPC-facing
//! `PrivacySubmitter` adapter stays in the assembly layer so this crate remains
//! rpc-free. Gated behind the `integration` feature (default off) to preserve
//! the pre-move default build surface.

use crate::{
    attestation::AttestationService,
    enclave::{EnclaveConfig, TeeEnclave},
    mempool::{
        CommitteeEpochKey, ContentEncryptionConfig, EncryptionMode, FairOrderingPolicy,
        KeyEncapsulation, TeeMempool, TeeMempoolConfig,
    },
    wallet::TeeWalletProvider,
};
use parking_lot::RwLock;
use std::io::ErrorKind;
use std::path::PathBuf;
use std::sync::{Arc, OnceLock, Weak};
use std::time::Duration;
use tracing::{info, warn};

/// Resolved privacy-mode encryption configuration (strictly opt-in).
///
/// Produced from the `[privacy]` config section; `None` everywhere else means
/// privacy mode is off and the pre-privacy behavior is kept byte-for-byte.
#[derive(Debug, Clone)]
pub enum PrivacyEncryption {
    /// Committee epoch key material explicitly provisioned via config.
    CommitteeKey { epoch_id: u64, secret: [u8; 32] },
    /// Enclave sealing key encapsulates payload keys (rollback-floor guarded).
    TeeSealedKey,
}

impl PrivacyEncryption {
    /// Resolves the `[privacy]` config section into an encryption mode.
    ///
    /// Fails closed on unknown modes or missing/invalid key material so that
    /// startup names exactly what is missing.
    pub fn resolve(section: &crate::config::PrivacySection) -> Result<Self, String> {
        match section.encryption_mode.as_str() {
            "committee_key" => {
                let hex_text = section
                    .committee_epoch_key_hex
                    .as_deref()
                    .map(str::trim)
                    .ok_or_else(|| {
                        "privacy: encryption_mode = \"committee_key\" requires \
                         committee_epoch_key_hex (64 hex chars)"
                            .to_string()
                    })?;
                let bytes = hex::decode(hex_text).map_err(|err| {
                    format!("privacy: committee_epoch_key_hex is not valid hex: {err}")
                })?;
                let secret: [u8; 32] = bytes.try_into().map_err(|bytes: Vec<u8>| {
                    format!(
                        "privacy: committee_epoch_key_hex must be exactly 32 bytes \
                         (64 hex chars), got {} bytes",
                        bytes.len()
                    )
                })?;
                Ok(Self::CommitteeKey {
                    epoch_id: section.committee_epoch_id.unwrap_or(1),
                    secret,
                })
            }
            "tee_sealed_key" => Ok(Self::TeeSealedKey),
            other => Err(format!(
                "privacy: unknown encryption_mode \"{other}\" \
                 (expected \"committee_key\" or \"tee_sealed_key\")"
            )),
        }
    }
}

/// TEE runtime state
pub struct TeeRuntime {
    pub enclave: Arc<TeeEnclave>,
    pub wallet_provider: TeeWalletProvider,
    pub mempool: TeeMempool,
    pub attestation: AttestationService,
    /// Resolved privacy-mode configuration; `None` keeps pre-privacy behavior.
    privacy: Option<PrivacyEncryption>,
    /// Client-facing key encapsulation used by the encrypted submission RPC
    /// endpoint. Present only when privacy mode is enabled.
    intake_kem: Option<Arc<KeyEncapsulation>>,
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
    /// Initialize the TEE runtime with the pre-privacy (plaintext mirroring)
    /// behavior. Byte-identical to the behavior before privacy mode existed.
    pub fn new(
        data_path: PathBuf,
        ordering_policy: &str,
        mempool_capacity: usize,
    ) -> Result<Self, crate::TeeError> {
        Self::build(data_path, ordering_policy, mempool_capacity, None)
    }

    /// Initialize the TEE runtime with privacy mode explicitly enabled.
    ///
    /// The mempool stores only ciphertext plus hashes; decrypted payloads are
    /// surfaced only after `resolve_pending` moves entries to `Resolved`.
    pub fn with_privacy(
        data_path: PathBuf,
        ordering_policy: &str,
        mempool_capacity: usize,
        privacy: PrivacyEncryption,
    ) -> Result<Self, crate::TeeError> {
        Self::build(data_path, ordering_policy, mempool_capacity, Some(&privacy))
    }

    fn build(
        data_path: PathBuf,
        ordering_policy: &str,
        mempool_capacity: usize,
        privacy: Option<&PrivacyEncryption>,
    ) -> Result<Self, crate::TeeError> {
        info!(target: "neo::tee", "Initializing TEE runtime");

        // Create enclave configuration
        let allow_simulation = std::env::var("NEO_TEE_ALLOW_SIMULATION")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let simulation = !cfg!(feature = "tee-sgx");
        if simulation && !cfg!(debug_assertions) && !allow_simulation {
            return Err(crate::TeeError::enclave_init_error(
                crate::EnclaveInitError::InvalidConfiguration,
                "TEE simulation mode is not allowed in release builds without NEO_TEE_ALLOW_SIMULATION=1",
            ));
        }

        let enclave_config = EnclaveConfig {
            sealed_data_path: data_path.clone(),
            debug_mode: cfg!(debug_assertions),
            heap_size_mb: 256,
            tcs_count: 4,
            simulation,
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

        // Create TEE mempool.
        //
        // Privacy-off path is byte-identical to the pre-privacy construction
        // (`encrypt_contents: false`, `TeeMempool::new`). Privacy-on paths
        // enable content encryption:
        // - `tee_sealed_key`: the enclave sealing key encapsulates payload keys.
        // - `committee_key`: neo-tee provisions a fresh random epoch key per
        //   start (restart = new epoch) for the mempool-internal mirror; the
        //   client-facing intake KEM below uses the explicitly configured
        //   `committee_epoch_key_hex` secret.
        let mempool_config = TeeMempoolConfig {
            capacity: mempool_capacity,
            ordering_policy: policy,
            batch_interval: Duration::from_millis(100),
            encrypt_contents: privacy.is_some(),
        };
        let mempool = match privacy {
            Some(PrivacyEncryption::CommitteeKey { epoch_id, .. }) => {
                TeeMempool::with_content_encryption(
                    enclave.clone(),
                    mempool_config,
                    ContentEncryptionConfig {
                        mode: EncryptionMode::CommitteeKey {
                            epoch_id: *epoch_id,
                        },
                    },
                )?
            }
            _ => TeeMempool::new(enclave.clone(), mempool_config)?,
        };

        // Client-facing intake KEM for the encrypted submission endpoint.
        let intake_kem = privacy.map(|mode| match mode {
            PrivacyEncryption::CommitteeKey { epoch_id, secret } => Arc::new(
                KeyEncapsulation::CommitteeKey(CommitteeEpochKey::from_secret(*epoch_id, *secret)),
            ),
            PrivacyEncryption::TeeSealedKey => {
                Arc::new(KeyEncapsulation::TeeSealed(enclave.clone()))
            }
        });

        // Create wallet provider
        let wallet_provider = TeeWalletProvider::new(enclave.clone())?;

        // Create attestation service
        let attestation = AttestationService::new(enclave.clone())?;

        if let Some(mode) = privacy {
            info!(
                target: "neo::tee",
                mode = ?mode,
                "TEE runtime initialized with privacy mode ENABLED (encrypted mempool contents)"
            );
        } else {
            info!(target: "neo::tee", "TEE runtime initialized successfully");
        }

        Ok(Self {
            enclave,
            wallet_provider,
            mempool,
            attestation,
            privacy: privacy.cloned(),
            intake_kem,
        })
    }

    /// Whether privacy mode is explicitly enabled on this runtime.
    pub fn privacy_enabled(&self) -> bool {
        self.privacy.is_some()
    }

    /// The client-facing intake key encapsulation, present only when privacy
    /// mode is enabled. Consumed by the privacy tests and future callers
    /// (e.g. epoch rotation tooling).
    #[cfg_attr(not(test), allow(dead_code))]
    pub fn intake_kem(&self) -> Option<&Arc<KeyEncapsulation>> {
        self.intake_kem.as_ref()
    }

    /// Generate attestation report for the TEE
    pub fn generate_attestation(&self) -> Result<Vec<u8>, crate::TeeError> {
        let report = self.attestation.generate_report(b"neo-node-attestation")?;
        Ok(report.to_bytes())
    }

    /// Shutdown the TEE runtime
    pub fn shutdown(&self) -> Result<(), crate::TeeError> {
        info!(target: "neo::tee", "Shutting down TEE runtime");
        self.enclave.shutdown()
    }

    /// Runs startup self-checks to ensure core TEE logic is operational.
    ///
    /// This validates:
    /// - TEE mempool add/order/proof path
    /// - TEE wallet key creation + signature path
    pub fn run_startup_self_checks(&self) -> Result<(), crate::TeeError> {
        let tx_hash: [u8; 32] = rand::random();
        let sender: [u8; 20] = rand::random();
        let payload = b"neo-node-tee-startup-self-check".to_vec();

        self.mempool
            .add_transaction(tx_hash, payload, 1_000, 0, sender)?;

        // Privacy mode: entries stay Encrypted until the TEE decrypt state
        // machine resolves them, so run it before the ordering check.
        if self.privacy_enabled() {
            let summary = self.mempool.resolve_pending();
            if summary.resolved != 1 || summary.rejected != 0 {
                return Err(crate::TeeError::OrderingError(format!(
                    "TEE mempool encrypted self-check failed: resolved {}, rejected {}",
                    summary.resolved, summary.rejected
                )));
            }
        }

        let ordered = self.mempool.get_ordered_hashes(1);
        if ordered.first().copied() != Some(tx_hash) {
            return Err(crate::TeeError::OrderingError(
                "TEE mempool ordering self-check failed".to_string(),
            ));
        }

        let proof = self.mempool.generate_ordering_proof()?;
        if proof.public_key.is_empty() || proof.signature.is_empty() {
            return Err(crate::TeeError::OrderingError(
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
                            return Err(crate::TeeError::Other(format!(
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
                        return Err(crate::TeeError::Other(format!(
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

fn run_wallet_signing_self_check(wallet: &crate::TeeWallet) -> Result<(), crate::TeeError> {
    if wallet.list_keys().is_empty() {
        let _ = wallet.create_key(Some("startup-self-check".to_string()))?;
    }

    let default_key = wallet
        .default_account()
        .or_else(|| wallet.list_keys().into_iter().next())
        .ok_or_else(|| {
            crate::TeeError::Other("TEE wallet self-check did not produce any keys".to_string())
        })?;
    let signature = wallet.sign(
        &default_key.script_hash,
        b"neo-node-tee-signature-self-check",
    )?;
    if signature.is_empty() {
        return Err(crate::TeeError::Other(
            "TEE wallet self-check produced an empty signature".to_string(),
        ));
    }

    Ok(())
}

fn is_stale_startup_wallet_error(err: &crate::TeeError) -> bool {
    matches!(
        err,
        crate::TeeError::UnsealingFailed(_) | crate::TeeError::CryptoError(_)
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
                    std::assert_matches!(
                        err,
                        crate::TeeError::EnclaveInitError { .. },
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
                    std::assert_matches!(
                        err,
                        crate::TeeError::EnclaveInitError { .. },
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
        std::assert_matches!(
            parse_ordering_policy("fcfs"),
            FairOrderingPolicy::FirstComeFirstServed
        );
        std::assert_matches!(
            parse_ordering_policy("batched"),
            FairOrderingPolicy::BatchedRandom { .. }
        );
        std::assert_matches!(
            parse_ordering_policy("commit-reveal"),
            FairOrderingPolicy::CommitReveal { .. }
        );
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

    // ------------------------------------------------------------------
    // Privacy mode (strictly opt-in) — default-off and enabled-on behavior
    // ------------------------------------------------------------------

    use crate::mempool::{ContentState, MODE_TEE_SEALED_KEY, MempoolCiphertext};

    #[test]
    fn privacy_encryption_resolve_validates_fail_closed() {
        let mut section = crate::config::PrivacySection::default();
        assert!(!section.enabled);
        assert!(matches!(
            PrivacyEncryption::resolve(&section).unwrap(),
            PrivacyEncryption::TeeSealedKey
        ));

        section.encryption_mode = "committee_key".to_string();
        let err = PrivacyEncryption::resolve(&section).unwrap_err();
        assert!(err.contains("committee_epoch_key_hex"), "unexpected: {err}");

        section.committee_epoch_key_hex = Some("zz".to_string());
        let err = PrivacyEncryption::resolve(&section).unwrap_err();
        assert!(err.contains("hex"), "unexpected: {err}");

        section.committee_epoch_key_hex = Some("ab".repeat(31));
        let err = PrivacyEncryption::resolve(&section).unwrap_err();
        assert!(err.contains("32 bytes"), "unexpected: {err}");

        section.committee_epoch_key_hex = Some("ab".repeat(33));
        let err = PrivacyEncryption::resolve(&section).unwrap_err();
        assert!(err.contains("32 bytes"), "unexpected: {err}");

        section.committee_epoch_key_hex = Some("ab".repeat(32));
        section.committee_epoch_id = Some(5);
        match PrivacyEncryption::resolve(&section).unwrap() {
            PrivacyEncryption::CommitteeKey { epoch_id, .. } => assert_eq!(epoch_id, 5),
            other => panic!("unexpected mode: {other:?}"),
        }

        section.encryption_mode = "bogus".to_string();
        let err = PrivacyEncryption::resolve(&section).unwrap_err();
        assert!(err.contains("unknown encryption_mode"), "unexpected: {err}");
    }

    #[cfg(not(feature = "tee-sgx"))]
    #[test]
    fn privacy_mode_tee_sealed_stores_only_ciphertext() {
        let temp = tempdir().unwrap();
        let runtime = TeeRuntime::with_privacy(
            temp.path().to_path_buf(),
            "fcfs",
            1000,
            PrivacyEncryption::TeeSealedKey,
        )
        .expect("TEE runtime with tee-sealed privacy");

        assert!(runtime.privacy_enabled());
        assert!(runtime.intake_kem().is_some());

        let tx_hash = [3u8; 32];
        let payload = b"privacy-encrypted-intake".to_vec();
        runtime
            .mempool
            .add_transaction(tx_hash, payload.clone(), 1_000, 0, [7u8; 20])
            .unwrap();

        // Host-visible storage is only the ciphertext envelope, never the
        // plaintext payload.
        let stored = runtime
            .mempool
            .get_transaction(&tx_hash)
            .expect("stored entry");
        let envelope =
            MempoolCiphertext::decode(&stored).expect("stored bytes are a ciphertext envelope");
        assert_eq!(envelope.mode, MODE_TEE_SEALED_KEY);
        assert!(
            !stored
                .windows(payload.len())
                .any(|w| w == payload.as_slice())
        );

        // Encrypted entries are invisible to candidate ordering until the TEE
        // decrypt state machine resolves them.
        assert!(runtime.mempool.get_ordered_hashes(10).is_empty());
        assert_eq!(
            runtime.mempool.content_state(&tx_hash),
            Some(ContentState::Encrypted)
        );

        let summary = runtime.mempool.resolve_pending();
        assert_eq!(summary.resolved, 1);
        assert_eq!(summary.rejected, 0);
        assert_eq!(runtime.mempool.get_ordered_hashes(10), vec![tx_hash]);
        assert_eq!(
            runtime
                .mempool
                .get_resolved_transaction(&tx_hash)
                .as_deref(),
            Some(payload.as_slice())
        );
    }

    #[cfg(not(feature = "tee-sgx"))]
    #[test]
    fn privacy_mode_rejected_entry_fails_closed_out_of_candidates() {
        let temp = tempdir().unwrap();
        let runtime = TeeRuntime::with_privacy(
            temp.path().to_path_buf(),
            "fcfs",
            1000,
            PrivacyEncryption::TeeSealedKey,
        )
        .expect("TEE runtime with tee-sealed privacy");

        let tx1_hash = [1u8; 32];
        let tx2_hash = [2u8; 32];
        let payload = b"privacy-rejected-entry".to_vec();

        runtime
            .mempool
            .add_transaction(tx1_hash, payload.clone(), 1_000, 0, [7u8; 20])
            .unwrap();
        runtime
            .mempool
            .add_transaction(tx2_hash, payload.clone(), 1_000, 0, [7u8; 20])
            .unwrap();

        assert_eq!(
            runtime.mempool.content_state(&tx1_hash),
            Some(ContentState::Encrypted)
        );
        assert_eq!(
            runtime.mempool.content_state(&tx2_hash),
            Some(ContentState::Encrypted)
        );

        // Simulate a partial rollback of the floor witness: tx1 burned counter
        // 1 and tx2 counter 2, so a floor of 1 still covers tx1 while tx2 is no
        // longer covered and must fail closed at decapsulation time.
        let floor = crate::enclave::rollback_floor::floor_path(temp.path());
        crate::enclave::rollback_floor::store(&floor, 1).expect("floor store");

        let summary = runtime.mempool.resolve_pending();
        assert_eq!(summary.resolved, 1);
        assert_eq!(summary.rejected, 1);

        // Only the resolved entry enters the candidate ordering.
        assert_eq!(runtime.mempool.get_ordered_hashes(10), vec![tx1_hash]);
        // Rejected fail closed: no plaintext is surfaced and the entry is
        // reported through the rejected list instead.
        assert_eq!(runtime.mempool.get_resolved_transaction(&tx2_hash), None);
        assert_eq!(runtime.mempool.rejected_hashes(), vec![tx2_hash]);
    }

    #[cfg(not(feature = "tee-sgx"))]
    #[test]
    fn privacy_startup_self_checks_pass_when_enabled() {
        for privacy in [
            PrivacyEncryption::TeeSealedKey,
            PrivacyEncryption::CommitteeKey {
                epoch_id: 1,
                secret: [0x33u8; 32],
            },
        ] {
            let temp = tempdir().unwrap();
            let runtime =
                TeeRuntime::with_privacy(temp.path().to_path_buf(), "batched", 1000, privacy)
                    .expect("TEE runtime with privacy");
            runtime
                .run_startup_self_checks()
                .expect("privacy-mode startup self-checks pass");
        }
    }
}
