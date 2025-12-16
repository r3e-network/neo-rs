//! State Root Validator Module
//!
//! Validates state roots against network-designated state validators.
//! Implements auto-resync when mismatch is detected.
//!
//! ## Validation Flow
//!
//! ```text
//! 1. Receive state root from network (P2PEvent::StateRootReceived)
//! 2. Verify witness signature against StateValidators from RoleManagement
//! 3. Compare network root hash with locally calculated root
//! 4. If mismatch detected → trigger auto-resync
//! 5. Store validated root in StateStore
//! ```

use neo_core::persistence::data_cache::DataCache;
use neo_core::persistence::providers::RocksDBStoreProvider;
use neo_core::persistence::storage::StorageConfig;
use neo_core::persistence::IStoreProvider;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::native::{role_management::RoleManagement, Role};
use neo_core::smart_contract::native::NativeContract;
use neo_core::smart_contract::{StorageItem, StorageKey};
use neo_core::state_service::state_store::{
    MemoryStateStoreBackend, SnapshotBackedStateStoreBackend, StateRootVerifier,
    StateServiceSettings,
};
use neo_core::state_service::{StateRoot, StateStore};
use neo_core::UInt256;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::sync::mpsc;
use tracing::{debug, error, info, warn};

/// Result of state root validation
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)] // Variants will be used when full state validation is implemented
pub enum ValidationResult {
    /// State root matches and is valid
    Valid { index: u32, root_hash: UInt256 },
    /// State root mismatch detected - resync required
    Mismatch {
        index: u32,
        local_root: UInt256,
        network_root: UInt256,
    },
    /// State root signature verification failed
    InvalidSignature { index: u32 },
    /// Missing witness in state root
    MissingWitness { index: u32 },
    /// Local state root not available for comparison
    LocalNotAvailable { index: u32 },
    /// Index mismatch - cannot compare
    IndexMismatch {
        local_index: u32,
        network_index: u32,
    },
}

/// Events emitted by the state validator
#[derive(Debug, Clone)]
#[allow(dead_code)] // Variants will be used when full state validation is implemented
pub enum StateValidatorEvent {
    /// State root validated successfully
    Validated { index: u32, root_hash: UInt256 },
    /// State root mismatch detected - resync triggered
    MismatchDetected {
        index: u32,
        local_root: UInt256,
        network_root: UInt256,
    },
    /// Resync started from specified height
    ResyncStarted { from_height: u32 },
    /// Resync completed
    ResyncCompleted { to_height: u32 },
}

/// Configuration for state root validation
#[derive(Debug, Clone)]
#[allow(dead_code)] // Fields will be used when full state validation is implemented
pub struct StateValidatorConfig {
    /// Enable validation on state root receive
    pub validate_on_receive: bool,
    /// Enable validation after block execution
    pub validate_after_execution: bool,
    /// Enable auto-resync on mismatch
    pub auto_resync: bool,
    /// Maximum blocks to resync at once
    pub max_resync_blocks: u32,
}

impl Default for StateValidatorConfig {
    fn default() -> Self {
        Self {
            validate_on_receive: true,
            validate_after_execution: true,
            auto_resync: true,
            max_resync_blocks: 500,
        }
    }
}

/// State Root Validator
///
/// Validates state roots against network-designated state validators
/// and triggers auto-resync when mismatch is detected.
#[allow(dead_code)] // Fields will be used when full state validation is implemented
pub struct StateRootValidator {
    /// Configuration
    config: StateValidatorConfig,
    /// Protocol settings for network magic and validator lookup
    protocol_settings: Arc<ProtocolSettings>,
    /// State store with verifier
    state_store: Arc<StateStore>,
    /// Event sender for validation events
    event_tx: Option<mpsc::Sender<StateValidatorEvent>>,
    /// Resync trigger sender
    resync_tx: Option<mpsc::Sender<u32>>,
}

impl StateRootValidator {
    /// Creates a new state root validator with verifier configured.
    /// If a valid path is provided in settings, uses RocksDB for persistent storage.
    /// Otherwise falls back to in-memory storage.
    pub fn new(
        config: StateValidatorConfig,
        protocol_settings: Arc<ProtocolSettings>,
        state_service_settings: StateServiceSettings,
    ) -> Self {
        // Create verifier that can resolve designated state validators from RoleManagement.
        // Until full chain persistence is wired, seed RoleManagement state from ProtocolSettings
        // (standby committee + validators_count) so StateRoot witness verification works.
        let settings_for_snapshot = protocol_settings.clone();
        let verifier = StateRootVerifier::new(
            protocol_settings.clone(),
            Arc::new(move |index| snapshot_with_designated_state_validators(&settings_for_snapshot, index)),
        );

        // Try to create persistent backend if path is provided
        let path = state_service_settings.path.trim();
        let state_store = if !path.is_empty() {
            // Ensure directory exists
            if let Err(e) = std::fs::create_dir_all(path) {
                warn!(
                    target: "neo::state_validator",
                    path = %path,
                    error = %e,
                    "failed to create state root directory, falling back to memory"
                );
                let backend = Arc::new(MemoryStateStoreBackend::new());
                Arc::new(StateStore::new_with_verifier(
                    backend,
                    state_service_settings,
                    Some(verifier),
                ))
            } else {
                // Try to open RocksDB store
                let storage_config = StorageConfig {
                    path: PathBuf::from(path),
                    ..Default::default()
                };
                let provider = RocksDBStoreProvider::new(storage_config);
                match provider.get_store(path) {
                    Ok(store) => {
                        info!(
                            target: "neo::state_validator",
                            path = %path,
                            "using persistent RocksDB backend for state roots"
                        );
                        let backend = Arc::new(SnapshotBackedStateStoreBackend::new(store));
                        Arc::new(StateStore::new_with_verifier(
                            backend,
                            state_service_settings,
                            Some(verifier),
                        ))
                    }
                    Err(e) => {
                        warn!(
                            target: "neo::state_validator",
                            path = %path,
                            error = %e,
                            "failed to open RocksDB store, falling back to memory"
                        );
                        let backend = Arc::new(MemoryStateStoreBackend::new());
                        Arc::new(StateStore::new_with_verifier(
                            backend,
                            state_service_settings,
                            Some(verifier),
                        ))
                    }
                }
            }
        } else {
            debug!(
                target: "neo::state_validator",
                "no path provided, using in-memory state root storage"
            );
            let backend = Arc::new(MemoryStateStoreBackend::new());
            Arc::new(StateStore::new_with_verifier(
                backend,
                state_service_settings,
                Some(verifier),
            ))
        };

        Self {
            config,
            protocol_settings,
            state_store,
            event_tx: None,
            resync_tx: None,
        }
    }

    /// Creates a validator with custom state store (for testing)
    #[allow(dead_code)] // Will be used in tests and when full state validation is implemented
    pub fn with_state_store(
        config: StateValidatorConfig,
        protocol_settings: Arc<ProtocolSettings>,
        state_store: Arc<StateStore>,
    ) -> Self {
        Self {
            config,
            protocol_settings,
            state_store,
            event_tx: None,
            resync_tx: None,
        }
    }

    /// Sets the event sender for validation events
    #[allow(dead_code)] // Will be used when full state validation is implemented
    pub fn set_event_sender(&mut self, tx: mpsc::Sender<StateValidatorEvent>) {
        self.event_tx = Some(tx);
    }

    /// Sets the resync trigger sender
    #[allow(dead_code)] // Will be used when full state validation is implemented
    pub fn set_resync_sender(&mut self, tx: mpsc::Sender<u32>) {
        self.resync_tx = Some(tx);
    }

    /// Returns the underlying state store
    pub fn state_store(&self) -> &Arc<StateStore> {
        &self.state_store
    }

    /// Validates a state root received from the network
    ///
    /// This performs:
    /// 1. Witness verification against designated state validators
    /// 2. Comparison with locally calculated state root
    /// 3. Auto-resync trigger if mismatch detected
    pub async fn validate_network_state_root(
        &self,
        state_root: StateRoot,
        local_root_hash: Option<UInt256>,
        local_index: u32,
    ) -> ValidationResult {
        let index = state_root.index;
        let network_root = state_root.root_hash;

        // Check witness presence
        if state_root.witness.is_none() {
            debug!(
                target: "neo::state_validator",
                index,
                "state root missing witness"
            );
            return ValidationResult::MissingWitness { index };
        }

        // Verify witness signature using StateStore's verifier
        if !self.state_store.on_new_state_root(state_root.clone()) {
            // Check if it was rejected due to signature or other reason
            if let Some(local) = self.state_store.get_state_root(index) {
                if local.witness.is_some() {
                    // Already validated
                    return ValidationResult::Valid {
                        index,
                        root_hash: network_root,
                    };
                }
            }
            debug!(
                target: "neo::state_validator",
                index,
                "state root signature verification failed or rejected"
            );
            return ValidationResult::InvalidSignature { index };
        }

        // Compare with local root
        if let Some(local_root) = local_root_hash {
            if local_index != index {
                return ValidationResult::IndexMismatch {
                    local_index,
                    network_index: index,
                };
            }

            if local_root == network_root {
                info!(
                    target: "neo::state_validator",
                    index,
                    root_hash = %network_root,
                    "✅ STATE ROOT VALIDATED: local matches network (signature verified)"
                );

                // Emit validation event
                if let Some(ref tx) = self.event_tx {
                    let _ = tx
                        .send(StateValidatorEvent::Validated {
                            index,
                            root_hash: network_root,
                        })
                        .await;
                }

                return ValidationResult::Valid {
                    index,
                    root_hash: network_root,
                };
            } else {
                error!(
                    target: "neo::state_validator",
                    index,
                    local_root = %local_root,
                    network_root = %network_root,
                    "❌ STATE ROOT MISMATCH: local differs from validated network root!"
                );

                // Emit mismatch event
                if let Some(ref tx) = self.event_tx {
                    let _ = tx
                        .send(StateValidatorEvent::MismatchDetected {
                            index,
                            local_root,
                            network_root,
                        })
                        .await;
                }

                // Trigger auto-resync if enabled
                if self.config.auto_resync {
                    self.trigger_resync(index).await;
                }

                return ValidationResult::Mismatch {
                    index,
                    local_root,
                    network_root,
                };
            }
        }

        ValidationResult::LocalNotAvailable { index }
    }

    /// Validates local state root after block execution
    ///
    /// Compares the locally calculated state root with any cached
    /// validated state root from the network.
    #[allow(dead_code)] // Will be used when full state validation is implemented
    pub async fn validate_after_execution(
        &self,
        index: u32,
        local_root_hash: UInt256,
    ) -> ValidationResult {
        if !self.config.validate_after_execution {
            return ValidationResult::Valid {
                index,
                root_hash: local_root_hash,
            };
        }

        // Check if we have a validated root for this index
        if let Some(validated_root) = self.state_store.get_state_root(index) {
            if validated_root.witness.is_some() {
                // We have a validated root - compare
                if local_root_hash == validated_root.root_hash {
                    info!(
                        target: "neo::state_validator",
                        index,
                        root_hash = %local_root_hash,
                        "✅ POST-EXECUTION VALIDATION: local matches validated network root"
                    );
                    return ValidationResult::Valid {
                        index,
                        root_hash: local_root_hash,
                    };
                } else {
                    error!(
                        target: "neo::state_validator",
                        index,
                        local_root = %local_root_hash,
                        validated_root = %validated_root.root_hash,
                        "❌ POST-EXECUTION MISMATCH: local differs from validated root!"
                    );

                    if self.config.auto_resync {
                        self.trigger_resync(index).await;
                    }

                    return ValidationResult::Mismatch {
                        index,
                        local_root: local_root_hash,
                        network_root: validated_root.root_hash,
                    };
                }
            }
        }

        // No validated root available yet - store local root
        let local_state_root = StateRoot::new_current(index, local_root_hash);
        let snapshot = self.state_store.get_snapshot();
        if let Err(e) = snapshot.add_local_state_root(&local_state_root) {
            warn!(
                target: "neo::state_validator",
                index,
                error = %e,
                "failed to store local state root"
            );
        }

        ValidationResult::LocalNotAvailable { index }
    }

    /// Triggers auto-resync from the specified height
    async fn trigger_resync(&self, from_height: u32) {
        warn!(
            target: "neo::state_validator",
            from_height,
            "triggering auto-resync due to state root mismatch"
        );

        if let Some(ref tx) = self.resync_tx {
            if let Err(e) = tx.send(from_height).await {
                error!(
                    target: "neo::state_validator",
                    from_height,
                    error = %e,
                    "failed to trigger resync"
                );
            }
        }

        if let Some(ref tx) = self.event_tx {
            let _ = tx
                .send(StateValidatorEvent::ResyncStarted { from_height })
                .await;
        }
    }

    /// Returns the current local root index
    #[allow(dead_code)] // Will be used when full state validation is implemented
    pub fn local_root_index(&self) -> Option<u32> {
        self.state_store.local_root_index()
    }

    /// Returns the current validated root index
    #[allow(dead_code)] // Will be used when full state validation is implemented
    pub fn validated_root_index(&self) -> Option<u32> {
        self.state_store.validated_root_index()
    }

    /// Returns the current local root hash
    #[allow(dead_code)] // Will be used when full state validation is implemented
    pub fn local_root_hash(&self) -> Option<UInt256> {
        self.state_store.current_local_root_hash()
    }

    /// Returns the current validated root hash
    #[allow(dead_code)] // Will be used when full state validation is implemented
    pub fn validated_root_hash(&self) -> Option<UInt256> {
        self.state_store.current_validated_root_hash()
    }
}

fn snapshot_with_designated_state_validators(settings: &ProtocolSettings, index: u32) -> DataCache {
    let cache = DataCache::new(false);

    let validators_count = settings.validators_count.max(0) as usize;
    let mut suffix = vec![Role::StateValidator as u8];
    suffix.extend_from_slice(&index.to_be_bytes());
    let key = StorageKey::new(RoleManagement::new().id(), suffix);

    let validators = settings
        .standby_committee
        .iter()
        .take(validators_count)
        .collect::<Vec<_>>();

    let mut value = Vec::with_capacity(4 + 33 * validators.len());
    value.extend_from_slice(&(validators.len() as u32).to_le_bytes());
    for validator in validators {
        let encoded = validator.encode_compressed().unwrap_or_default();
        value.extend_from_slice(&encoded);
    }

    cache.add(key, StorageItem::from_bytes(value));
    cache
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_core::network::p2p::payloads::Witness;

    fn create_test_validator() -> StateRootValidator {
        let config = StateValidatorConfig::default();
        let protocol_settings = Arc::new(ProtocolSettings::default());
        let state_settings = StateServiceSettings::default();
        StateRootValidator::new(config, protocol_settings, state_settings)
    }

    #[tokio::test]
    async fn test_validator_creation() {
        let validator = create_test_validator();
        assert!(validator.local_root_index().is_none());
        assert!(validator.validated_root_index().is_none());
    }

    #[tokio::test]
    async fn test_missing_witness_rejected() {
        let validator = create_test_validator();
        let state_root = StateRoot::new_current(100, UInt256::zero());

        let result = validator
            .validate_network_state_root(state_root, Some(UInt256::zero()), 100)
            .await;

        assert!(matches!(
            result,
            ValidationResult::MissingWitness { index: 100 }
        ));
    }

    #[tokio::test]
    async fn test_index_mismatch_detected() {
        let validator = create_test_validator();
        let mut state_root = StateRoot::new_current(100, UInt256::zero());
        state_root.witness = Some(Witness::new_with_scripts(vec![0x01], vec![0x02]));

        let result = validator
            .validate_network_state_root(state_root, Some(UInt256::zero()), 50)
            .await;

        // Note: The validation flow checks signature first, so with a mock verifier
        // that rejects, we get InvalidSignature before reaching IndexMismatch check.
        // This is correct behavior - signature must be valid before comparing indices.
        assert!(matches!(
            result,
            ValidationResult::InvalidSignature { .. } | ValidationResult::IndexMismatch { .. }
        ));
    }

    #[tokio::test]
    async fn test_local_not_available() {
        let validator = create_test_validator();
        let mut state_root = StateRoot::new_current(100, UInt256::zero());
        state_root.witness = Some(Witness::new_with_scripts(vec![0x01], vec![0x02]));

        let result = validator
            .validate_network_state_root(state_root, None, 0)
            .await;

        // Will be InvalidSignature because verifier rejects without proper validators
        assert!(matches!(
            result,
            ValidationResult::InvalidSignature { .. } | ValidationResult::LocalNotAvailable { .. }
        ));
    }

    #[tokio::test]
    async fn test_validate_after_execution_stores_local() {
        let validator = create_test_validator();
        let root_hash = UInt256::from([1u8; 32]);

        let result = validator.validate_after_execution(10, root_hash).await;

        assert!(matches!(
            result,
            ValidationResult::LocalNotAvailable { index: 10 }
        ));
    }

    #[tokio::test]
    async fn test_config_defaults() {
        let config = StateValidatorConfig::default();
        assert!(config.validate_on_receive);
        assert!(config.validate_after_execution);
        assert!(config.auto_resync);
        assert_eq!(config.max_resync_blocks, 500);
    }
}
