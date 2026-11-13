//! TEE integration for neo-node
//!
//! This module provides TEE-protected wallet and mempool functionality.
//!
//! Note: This module is feature-gated and activated when the `tee` feature is enabled.

use neo_tee::{
    attestation::AttestationService,
    enclave::{EnclaveConfig, TeeEnclave},
    mempool::{
        CommitteeEpochKey, ContentCipher, ContentEncryptionConfig, EncryptionMode,
        FairOrderingPolicy, KeyEncapsulation, MempoolCiphertext, TeeMempool, TeeMempoolConfig,
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
    ) -> Result<Self, neo_tee::TeeError> {
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
    ) -> Result<Self, neo_tee::TeeError> {
        Self::build(data_path, ordering_policy, mempool_capacity, Some(&privacy))
    }

    fn build(
        data_path: PathBuf,
        ordering_policy: &str,
        mempool_capacity: usize,
        privacy: Option<&PrivacyEncryption>,
    ) -> Result<Self, neo_tee::TeeError> {
        info!(target: "neo::tee", "Initializing TEE runtime");

        // Create enclave configuration
        let allow_simulation = std::env::var("NEO_TEE_ALLOW_SIMULATION")
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
        let simulation = !cfg!(feature = "tee-sgx");
        if simulation && !cfg!(debug_assertions) && !allow_simulation {
            return Err(neo_tee::TeeError::enclave_init_error(
                neo_tee::EnclaveInitError::InvalidConfiguration,
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

    /// Builds the host-side submitter for the encrypted RPC submission
    /// endpoint. Returns `None` when privacy mode is off.
    pub fn privacy_submitter(&self) -> Option<Arc<PrivacySubmitter>> {
        let kem = self.intake_kem.clone()?;
        let attestation = AttestationService::new(self.enclave.clone()).ok()?;
        Some(Arc::new(PrivacySubmitter { kem, attestation }))
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

        // Privacy mode: entries stay Encrypted until the TEE decrypt state
        // machine resolves them, so run it before the ordering check.
        if self.privacy_enabled() {
            let summary = self.mempool.resolve_pending();
            if summary.resolved != 1 || summary.rejected != 0 {
                return Err(neo_tee::TeeError::OrderingError(format!(
                    "TEE mempool encrypted self-check failed: resolved {}, rejected {}",
                    summary.resolved, summary.rejected
                )));
            }
        }

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

/// Host-side decrypting submitter backing the opt-in
/// `sendrawtransactionencrypted` RPC endpoint when privacy mode is enabled.
///
/// Holds the configured client-facing intake KEM plus the enclave attestation
/// service so submissions are answered with an attestation report containing
/// the enclave measurements.
pub struct PrivacySubmitter {
    kem: Arc<KeyEncapsulation>,
    attestation: AttestationService,
}

impl neo_rpc::server::EncryptedTransactionSubmitter for PrivacySubmitter {
    fn open_encrypted(
        &self,
        tx_hash: &neo_primitives::UInt256,
        ciphertext: &neo_rpc::server::MempoolCiphertext,
    ) -> Result<Arc<neo_protocol::payloads::transaction::Transaction>, String> {
        // The RPC layer hands over the raw wire bytes (base64-decoded
        // envelope). Decode them into the TEE ciphertext envelope before
        // decryption; malformed envelopes fail closed.
        let envelope =
            MempoolCiphertext::decode(ciphertext.as_bytes()).map_err(|err| err.to_string())?;
        let plaintext = ContentCipher::open(&envelope, &tx_hash.as_bytes(), &self.kem)
            .map_err(|err| err.to_string())?;
        let transaction = neo_protocol::payloads::transaction::Transaction::from_bytes(&plaintext)
            .map_err(|err| err.to_string())?;
        if transaction.hash() != *tx_hash {
            return Err("decrypted transaction hash does not match the claimed hash".to_string());
        }
        Ok(Arc::new(transaction))
    }

    fn attestation_report(&self) -> Result<Vec<u8>, String> {
        let report = self
            .attestation
            .generate_report(b"neo-node-privacy-submission")
            .map_err(|err| err.to_string())?;
        Ok(report.to_bytes())
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
                    std::assert_matches!(
                        err,
                        neo_tee::TeeError::EnclaveInitError { .. },
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
                        neo_tee::TeeError::EnclaveInitError { .. },
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

    use neo_primitives::UInt256;
    use neo_protocol::ProtocolSettings;
    use neo_protocol::payloads::transaction::Transaction;
    use neo_rpc::server::EncryptedTransactionSubmitter;
    use neo_tee::attestation::AttestationReport;
    use neo_tee::mempool::{ContentState, MODE_TEE_SEALED_KEY};
    use neo_wallets::KeyPair;

    fn build_privacy_test_transaction(keypair: &KeyPair) -> Transaction {
        use neo_primitives::WitnessScope;
        use neo_protocol::get_sign_data_vec;
        use neo_protocol::payloads::Signer;
        use neo_protocol::witness::Witness;
        use neo_vm::OpCode;

        let settings = ProtocolSettings::default();
        let mut tx = Transaction::new();
        tx.set_nonce(1);
        tx.set_network_fee(1_0000_0000);
        tx.set_system_fee(0);
        tx.set_valid_until_block(1);
        tx.set_script(vec![OpCode::PUSH1.byte()]);
        tx.set_signers(vec![Signer::new(
            keypair.get_script_hash(),
            WitnessScope::GLOBAL,
        )]);

        let sign_data = get_sign_data_vec(&tx, settings.network).expect("sign data");
        let signature = keypair.sign(&sign_data).expect("sign");
        let mut invocation = Vec::with_capacity(signature.len() + 2);
        invocation.push(OpCode::PUSHDATA1.byte());
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(&signature);
        tx.set_witnesses(vec![Witness::new_with_scripts(
            invocation,
            keypair.get_verification_script(),
        )]);
        tx
    }

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
    fn privacy_disabled_by_default_keeps_plaintext_runtime() {
        let temp = tempdir().unwrap();
        let runtime = TeeRuntime::new(temp.path().to_path_buf(), "batched", 1000)
            .expect("TEE runtime (privacy off)");

        // No encryption engine is installed: no privacy flag, no intake KEM,
        // no encrypted-submission submitter.
        assert!(!runtime.privacy_enabled());
        assert!(runtime.intake_kem().is_none());
        assert!(runtime.privacy_submitter().is_none());

        // Pre-privacy path: a mirrored transaction is immediately ordered
        // without any resolve step.
        let tx_hash = [4u8; 32];
        runtime
            .mempool
            .add_transaction(tx_hash, b"plain".to_vec(), 1_000, 0, [7u8; 20])
            .unwrap();
        assert_eq!(runtime.mempool.get_ordered_hashes(1), vec![tx_hash]);
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
        let floor = neo_tee::enclave::rollback_floor::floor_path(temp.path());
        neo_tee::enclave::rollback_floor::store(&floor, 1).expect("floor store");

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
    fn privacy_mode_committee_intake_kem_uses_configured_secret() {
        let temp = tempdir().unwrap();
        let runtime = TeeRuntime::with_privacy(
            temp.path().to_path_buf(),
            "batched",
            1000,
            PrivacyEncryption::CommitteeKey {
                epoch_id: 9,
                secret: [0x21u8; 32],
            },
        )
        .expect("TEE runtime with committee privacy");

        assert!(runtime.privacy_enabled());
        let submitter = runtime.privacy_submitter().expect("privacy submitter");
        let kem = runtime.intake_kem().expect("intake kem");

        let keypair = KeyPair::from_private_key(&[0x44u8; 32]).expect("keypair");
        let tx = build_privacy_test_transaction(&keypair);
        let ciphertext =
            ContentCipher::seal(&tx.to_bytes(), &tx.hash().as_bytes(), kem).expect("seal");
        // Wrap the encoded envelope into the RPC wire-format handle exactly as
        // the RPC endpoint would after base64 decoding.
        let wire_ciphertext = neo_rpc::server::MempoolCiphertext::from_wire_bytes(
            ciphertext.encode().expect("encode envelope"),
        );

        // Roundtrip: the configured committee secret decrypts the envelope.
        let opened = submitter
            .open_encrypted(&tx.hash(), &wire_ciphertext)
            .expect("open encrypted");
        assert_eq!(opened.hash(), tx.hash());

        // A wrong claimed hash fails closed (AEAD binds the hash as AAD).
        let wrong_hash = UInt256::from_bytes(&[0xABu8; 32]).unwrap();
        assert!(
            submitter
                .open_encrypted(&wrong_hash, &wire_ciphertext)
                .is_err()
        );

        // The attestation report carries enclave measurements.
        let report = submitter.attestation_report().expect("report");
        let parsed = AttestationReport::from_bytes(&report).expect("parse report");
        assert_ne!(parsed.mrenclave, [0u8; 32]);
        assert_ne!(parsed.mrsigner, [0u8; 32]);
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
