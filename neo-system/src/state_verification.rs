//! State verification utilities for the Neo blockchain.
//!
//! State roots are loaded from the same records written by the state-service
//! commit handlers. Verification fails closed when a root is absent, belongs to
//! a future height, differs from the expected hash, or carries an invalid
//! validator witness.

use neo_crypto::mpt_trie::{MptResult, MptStoreSnapshot, Trie};
use neo_primitives::UInt256;
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_storage::persistence::store::Store;
use neo_storage::persistence::{DataCache, SeekDirection, StoreCache};
use neo_storage::types::StorageKey;
use neo_wallets::signing::TrustedSignerClient;
use parking_lot::Mutex;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use crate::NeoSystem;
use crate::i_event_handlers::WalletChangedHandler;
use crate::state_service::StateRoot;
use crate::state_service::commit_handlers::{
    PREFIX_STATE_ROOT, STATE_ROOT_STORAGE_ID, deserialize_state_root_record, state_root_record_key,
};

/// Error type for state verification operations.
#[derive(Debug, Clone)]
pub struct StateVerificationError {
    pub message: String,
}

impl StateVerificationError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl std::fmt::Display for StateVerificationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "State verification error: {}", self.message)
    }
}

impl std::error::Error for StateVerificationError {}

/// Verifies that an MPT inclusion proof resolves `key` beneath `root`.
///
/// The proof nodes are loaded into an isolated store and checked by the same
/// MPT verifier used for state proofs. Malformed nodes, a broken hash chain,
/// and a missing key all return `false`.
#[must_use]
pub fn verify_merkle_proof(root: &UInt256, key: &[u8], proof: &[Vec<u8>]) -> bool {
    let proof_set: HashSet<Vec<u8>> = proof.iter().cloned().collect();

    #[derive(Default)]
    struct ProofStore {
        data: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
    }

    impl MptStoreSnapshot for ProofStore {
        fn try_get(&self, key: &[u8]) -> MptResult<Option<Vec<u8>>> {
            Ok(self.data.lock().get(key).cloned())
        }

        fn put(&self, key: Vec<u8>, value: Vec<u8>) -> MptResult<()> {
            self.data.lock().insert(key, value);
            Ok(())
        }

        fn delete(&self, key: Vec<u8>) -> MptResult<()> {
            self.data.lock().remove(&key);
            Ok(())
        }
    }

    Trie::<ProofStore>::verify_proof(*root, key, &proof_set).is_ok()
}

/// Read-side abstraction over persisted state roots and their verification
/// context.
///
/// `current_index` is the highest valid persisted root index, not the best
/// header height. This prevents a header-syncing node from treating a state
/// root that has not been persisted yet as verified.
pub trait StateRootSource {
    /// Loads the state-root record at `index`, returning `None` when it is
    /// missing or malformed.
    fn state_root_at(&self, index: u32) -> Option<StateRoot>;

    /// Returns the highest valid persisted state-root index.
    fn current_index(&self) -> Option<u32>;

    /// Protocol settings used for network-bound witness verification.
    fn protocol_settings(&self) -> &ProtocolSettings;

    /// Returns a storage snapshot used to resolve state-validator designations.
    fn verification_snapshot(&self) -> DataCache;
}

/// Production [`StateRootSource`] backed by the node's primary store.
pub struct StoreStateRootSource {
    store: Arc<dyn Store>,
    settings: ProtocolSettings,
}

impl StoreStateRootSource {
    /// Creates a source over the canonical state-root storage layout.
    #[must_use]
    pub fn new(store: Arc<dyn Store>, settings: ProtocolSettings) -> Self {
        Self { store, settings }
    }
}

impl StateRootSource for StoreStateRootSource {
    fn state_root_at(&self, index: u32) -> Option<StateRoot> {
        self.store
            .try_get(&state_root_record_key(index))
            .and_then(|item| deserialize_state_root_record(index, item.value()))
    }

    fn current_index(&self) -> Option<u32> {
        let prefix = StorageKey::new(STATE_ROOT_STORAGE_ID, vec![PREFIX_STATE_ROOT]);
        let mut highest = None;
        for (key, item) in self.store.find(Some(&prefix), SeekDirection::Forward) {
            let raw = key.key();
            if raw.len() != 5 || raw[0] != PREFIX_STATE_ROOT {
                continue;
            }
            let index = u32::from_be_bytes([raw[1], raw[2], raw[3], raw[4]]);
            if deserialize_state_root_record(index, item.value()).is_some()
                && highest.is_none_or(|current| index > current)
            {
                highest = Some(index);
            }
        }
        highest
    }

    fn protocol_settings(&self) -> &ProtocolSettings {
        &self.settings
    }

    fn verification_snapshot(&self) -> DataCache {
        StoreCache::new_from_store(self.store.clone(), true)
            .data_cache()
            .clone_cache()
    }
}

/// Verifies a persisted state root at `height` against `expected_root`.
///
/// An unsigned local record is verified by exact hash equality. A record that
/// carries a witness additionally has to pass the designated state-validator
/// quorum and signature checks in [`StateRoot::verify`].
pub fn verify_state_root(
    source: &dyn StateRootSource,
    height: u32,
    expected_root: &UInt256,
) -> Result<(), StateVerificationError> {
    if expected_root.is_zero() {
        return Err(StateVerificationError::new(format!(
            "invalid state root: zero hash at height {height}"
        )));
    }

    let current_index = source.current_index().ok_or_else(|| {
        StateVerificationError::new("no valid persisted state roots are available")
    })?;
    if height > current_index {
        return Err(StateVerificationError::new(format!(
            "state root height {height} exceeds current persisted height {current_index}"
        )));
    }

    let state_root = source.state_root_at(height).ok_or_else(|| {
        StateVerificationError::new(format!("no valid state root found at height {height}"))
    })?;
    if state_root.root_hash != *expected_root {
        return Err(StateVerificationError::new(format!(
            "state root mismatch at height {height}: expected {expected_root}, got {}",
            state_root.root_hash
        )));
    }

    if state_root.witness.is_some()
        && !state_root.verify(source.protocol_settings(), &source.verification_snapshot())
    {
        return Err(StateVerificationError::new(format!(
            "state root witness verification failed at height {height}"
        )));
    }

    Ok(())
}

/// Gates state-root verification on wallet availability.
///
/// Constructing this service requires the startup-bound trusted signer. The
/// wallet-changed callback controls whether verification is armed; calls made
/// while no wallet is available are rejected instead of being reported as
/// successful.
pub struct StateServiceVerification {
    system: Arc<NeoSystem>,
    armed: AtomicBool,
    _signer: TrustedSignerClient,
}

impl StateServiceVerification {
    /// Creates a disabled verification handler. A wallet-open callback arms it.
    #[must_use]
    pub fn new(system: Arc<NeoSystem>, signer: TrustedSignerClient) -> Self {
        Self {
            system,
            armed: AtomicBool::new(false),
            _signer: signer,
        }
    }

    /// Returns whether wallet-backed state-root verification is enabled.
    #[must_use]
    pub fn is_armed(&self) -> bool {
        self.armed.load(Ordering::Acquire)
    }

    /// Verifies a root from the node's primary store while the service is armed.
    pub fn verify_root(
        &self,
        height: u32,
        expected_root: &UInt256,
    ) -> Result<(), StateVerificationError> {
        if !self.is_armed() {
            return Err(StateVerificationError::new(
                "state root verification is not armed because no wallet is available",
            ));
        }
        let source = StoreStateRootSource::new(self.system.store(), self.system.settings().clone());
        verify_state_root(&source, height, expected_root)
    }
}

impl WalletChangedHandler for StateServiceVerification {
    fn wallet_provider_wallet_changed_handler(
        &self,
        _sender: &dyn std::any::Any,
        wallet: Option<Arc<dyn neo_wallets::Wallet>>,
    ) {
        let armed = wallet.is_some();
        self.armed.store(armed, Ordering::Release);
        tracing::info!(
            target: "neo::state",
            armed,
            "state root verification wallet availability changed"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_service::commit_handlers::serialize_state_root_record;
    use neo_native_contracts::{NativeContract, Role, role_management::RoleManagement};
    use neo_protocol::witness::Witness;
    use neo_smart_contract::Contract;
    use neo_storage::StorageItem;
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_vm::OpCode;
    use neo_wallets::signing::{
        AuthenticatedContext, FrozenSignIntent, SIGNING_SCHEMA_VERSION, SignRequest, SignResponse,
        SignResult, SignerService, SigningKeyResolver,
    };
    use neo_wallets::{KeyPair, Nep6Wallet};
    use std::collections::{BTreeMap, BTreeSet};
    use std::time::Duration;

    fn empty_store() -> Arc<dyn Store> {
        Arc::new(MemoryStore::new())
    }

    fn persist_bytes(store: &Arc<dyn Store>, key: StorageKey, value: Vec<u8>) {
        let mut cache = StoreCache::new_from_store(store.clone(), false);
        cache
            .data_cache()
            .try_add(key, StorageItem::from_bytes(value))
            .expect("stage storage record");
        cache.try_commit().expect("commit storage record");
    }

    fn persist_root(store: &Arc<dyn Store>, root: &StateRoot) {
        persist_bytes(
            store,
            state_root_record_key(root.index),
            serialize_state_root_record(root),
        );
    }

    fn unsigned_root(index: u32, byte: u8) -> StateRoot {
        StateRoot::new_current(index, UInt256::from([byte; 32]))
    }

    #[test]
    fn test_verify_merkle_proof_empty_proof() {
        assert!(!verify_merkle_proof(&UInt256::zero(), b"test_key", &[]));
    }

    #[test]
    fn test_verify_merkle_proof_invalid_proof() {
        assert!(!verify_merkle_proof(
            &UInt256::from([1u8; 32]),
            b"test_key",
            &[vec![1, 2, 3]],
        ));
    }

    #[test]
    fn test_verify_state_root_zero_hash() {
        let store = empty_store();
        let source = StoreStateRootSource::new(store, ProtocolSettings::default());
        let error = verify_state_root(&source, 0, &UInt256::zero()).unwrap_err();
        assert!(error.message.contains("zero hash"));
    }

    #[test]
    fn test_verify_state_root_accepts_matching_persisted_root() {
        let store = empty_store();
        let root = unsigned_root(7, 1);
        persist_root(&store, &root);
        let source = StoreStateRootSource::new(store, ProtocolSettings::default());

        assert_eq!(source.current_index(), Some(7));
        verify_state_root(&source, 7, &root.root_hash).expect("matching root should verify");
    }

    #[test]
    fn test_verify_state_root_rejects_future_missing_and_mismatched_roots() {
        let store = empty_store();
        let root = unsigned_root(3, 2);
        persist_root(&store, &root);
        let source = StoreStateRootSource::new(store.clone(), ProtocolSettings::default());

        let future = verify_state_root(&source, 4, &UInt256::from([3u8; 32])).unwrap_err();
        assert!(future.message.contains("exceeds current persisted height"));

        let missing = verify_state_root(&source, 2, &UInt256::from([3u8; 32])).unwrap_err();
        assert!(missing.message.contains("no valid state root"));

        let mismatch = verify_state_root(&source, 3, &UInt256::from([3u8; 32])).unwrap_err();
        assert!(mismatch.message.contains("state root mismatch"));

        let empty = StoreStateRootSource::new(empty_store(), ProtocolSettings::default());
        let absent = verify_state_root(&empty, 0, &UInt256::from([3u8; 32])).unwrap_err();
        assert!(absent.message.contains("no valid persisted state roots"));
    }

    #[test]
    fn test_store_source_ignores_malformed_records() {
        let store = empty_store();
        let root = unsigned_root(2, 4);
        persist_root(&store, &root);
        persist_bytes(&store, state_root_record_key(9), vec![1, 2, 3]);
        let source = StoreStateRootSource::new(store, ProtocolSettings::default());

        assert_eq!(source.current_index(), Some(2));
        assert!(source.state_root_at(9).is_none());
        verify_state_root(&source, 2, &root.root_hash).expect("valid lower root remains usable");
    }

    #[test]
    fn test_verify_state_root_rejects_invalid_witness() {
        let store = empty_store();
        let mut root = unsigned_root(4, 5);
        root.witness = Some(Witness::new_with_scripts(vec![1], vec![2]));
        persist_root(&store, &root);
        let source = StoreStateRootSource::new(store, ProtocolSettings::default());

        let error = verify_state_root(&source, 4, &root.root_hash).unwrap_err();
        assert!(error.message.contains("witness verification failed"));
    }

    #[test]
    fn test_verify_state_root_accepts_valid_validator_witness() {
        let store = empty_store();
        let mut settings = ProtocolSettings::mainnet();
        let keypair = KeyPair::generate().expect("validator keypair");
        let validator = keypair
            .get_public_key_point()
            .expect("validator public key");
        settings.standby_committee = vec![validator.clone()];
        settings.validators_count = 1;
        settings.network = 0x42_4242;
        let height: u32 = 5;

        let mut designation_key = vec![Role::StateValidator as u8];
        designation_key.extend_from_slice(&height.to_be_bytes());
        persist_bytes(
            &store,
            StorageKey::new(RoleManagement::new().id(), designation_key),
            crate::state_service::test_support::serialize_role_public_keys(std::slice::from_ref(
                &validator,
            )),
        );

        let mut root = unsigned_root(height, 6);
        let hash = root.hash();
        let mut sign_data = Vec::with_capacity(4 + hash.to_bytes().len());
        sign_data.extend_from_slice(&settings.network.to_le_bytes());
        sign_data.extend_from_slice(&hash.to_array());
        let signature = keypair.sign(&sign_data).expect("sign state root");
        let mut invocation = Vec::with_capacity(signature.len() + 2);
        invocation.push(OpCode::PUSHDATA1.byte());
        invocation.push(signature.len() as u8);
        invocation.extend_from_slice(&signature);
        root.witness = Some(Witness::new_with_scripts(
            invocation,
            Contract::create_multi_sig_redeem_script(1, &[validator]),
        ));
        persist_root(&store, &root);
        let source = StoreStateRootSource::new(store, settings);

        verify_state_root(&source, height, &root.root_hash)
            .expect("designated validator witness should verify");
    }

    #[test]
    fn test_state_verification_error_display() {
        let error = StateVerificationError::new("test error");
        assert_eq!(error.to_string(), "State verification error: test error");
    }

    struct RejectingSigner;

    #[async_trait::async_trait]
    impl SignerService for RejectingSigner {
        async fn sign(
            &self,
            _request: SignRequest,
            _context: AuthenticatedContext,
        ) -> SignResult<SignResponse> {
            unreachable!("root verification must not request a signature")
        }

        async fn resume_or_start(
            &self,
            _operation_id: &str,
            _frozen_intent: FrozenSignIntent,
            _context: AuthenticatedContext,
            _request_timeout: Duration,
        ) -> SignResult<SignResponse> {
            unreachable!("root verification must not start a signing operation")
        }
    }

    fn trusted_signer() -> TrustedSignerClient {
        TrustedSignerClient::new(
            Arc::new(RejectingSigner),
            "test-provider",
            "test-policy",
            Duration::from_secs(1),
            AuthenticatedContext {
                schema_version: SIGNING_SCHEMA_VERSION,
                principal: "state-verifier-test".to_string(),
                tenant: "state-verifier-test".to_string(),
                authentication_method: "test".to_string(),
                scopes: BTreeSet::new(),
                claims: BTreeMap::new(),
            },
            Arc::new(SigningKeyResolver::default()),
        )
        .expect("trusted signer")
    }

    #[tokio::test]
    async fn test_wallet_callback_controls_verification_call_chain() {
        let settings = ProtocolSettings::default();
        let system = NeoSystem::new(settings.clone(), None, None).expect("neo system");
        let verification = StateServiceVerification::new(system.clone(), trusted_signer());
        let root = unsigned_root(9, 7);
        persist_root(&system.store(), &root);

        let disabled = verification.verify_root(9, &root.root_hash).unwrap_err();
        assert!(disabled.message.contains("not armed"));

        let wallet: Arc<dyn neo_wallets::Wallet> = Arc::new(Nep6Wallet::create(
            Some("state-verifier".to_string()),
            None,
            Arc::new(settings),
        ));
        verification.wallet_provider_wallet_changed_handler(&(), Some(wallet));
        assert!(verification.is_armed());
        verification
            .verify_root(9, &root.root_hash)
            .expect("armed verification should read the persisted root");

        verification.wallet_provider_wallet_changed_handler(&(), None);
        assert!(!verification.is_armed());
        assert!(verification.verify_root(9, &root.root_hash).is_err());
    }
}
