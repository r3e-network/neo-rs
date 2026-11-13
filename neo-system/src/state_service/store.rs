//! Persistent state-root ingestion and progress tracking.

use super::StateRoot;
use super::commit_handlers::{
    PREFIX_STATE_ROOT, STATE_ROOT_STORAGE_ID, StateServiceError, deserialize_state_root_record,
    read_state_root_record, serialize_state_root_record, state_root_record_key,
};
use super::state_root::CURRENT_VERSION;
use crate::services::StateStoreService;
use neo_primitives::UInt256;
use neo_protocol::ProtocolSettings;
use neo_storage::persistence::store::Store;
use neo_storage::persistence::{DataCache, SeekDirection, StoreCache};
use neo_storage::{StorageItem, StorageKey};
use parking_lot::Mutex;
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use tracing::warn;

const PREFIX_LOCAL_ROOT_INDEX: u8 = 0xf2;
const PREFIX_VALIDATED_ROOT_INDEX: u8 = 0xf3;

/// Stores quorum-validated network roots and tracks locally committed roots.
///
/// Local and validated heights are deliberately independent: a signed root can
/// arrive before the corresponding block has been persisted locally. Progress
/// is durable even when a signed root arrives for an existing unsigned local
/// record, in which case the local record must remain intact.
pub(crate) struct StateRootStore {
    store: Arc<dyn Store>,
    store_cache: Arc<StoreCache>,
    settings: Arc<ProtocolSettings>,
    local_root_index: AtomicU64,
    validated_root_index: AtomicU64,
    reference_root_fetcher: Mutex<Option<Arc<dyn Fn(u32) -> Option<UInt256> + Send + Sync>>>,
    /// Roots staged in live block snapshots; entries are consumed on commit or rollback.
    staged_local_roots: Mutex<HashMap<u32, UInt256>>,
    write_lock: Mutex<()>,
}

impl StateRootStore {
    pub(crate) fn new(
        store: Arc<dyn Store>,
        store_cache: Arc<StoreCache>,
        settings: Arc<ProtocolSettings>,
    ) -> Result<Self, StateServiceError> {
        let local_index = recover_progress(&store_cache, PREFIX_LOCAL_ROOT_INDEX, false)?;
        let validated_index = recover_progress(&store_cache, PREFIX_VALIDATED_ROOT_INDEX, true)?;

        Ok(Self {
            store,
            store_cache,
            settings,
            local_root_index: AtomicU64::new(u64::from(local_index)),
            validated_root_index: AtomicU64::new(u64::from(validated_index)),
            reference_root_fetcher: Mutex::new(None),
            staged_local_roots: Mutex::new(HashMap::new()),
            write_lock: Mutex::new(()),
        })
    }

    fn stage_local_root_progress(&self, state_root: &StateRoot, snapshot: &DataCache) -> bool {
        if state_root.version != CURRENT_VERSION || state_root.witness.is_some() {
            return false;
        }
        if state_root.index <= self.local_root_index() as u32 {
            return true;
        }
        stage_progress_in_snapshot(snapshot, PREFIX_LOCAL_ROOT_INDEX, state_root.index).is_ok()
    }

    fn accept_local_root(&self, state_root: &StateRoot) -> bool {
        if state_root.version != CURRENT_VERSION || state_root.witness.is_some() {
            return false;
        }
        let Ok(Some(persisted)) = read_state_root_record(&self.store_cache, state_root.index)
        else {
            return false;
        };
        if persisted.version != state_root.version || persisted.root_hash != state_root.root_hash {
            return false;
        }
        if state_root.index <= self.local_root_index() as u32 {
            return true;
        }

        // The canonical block pipeline stages the cursor atomically with the root.
        // Older embedders may still notify after committing only the root record;
        // retain their repairable fallback write instead of silently losing progress.
        let cursor_is_committed = read_progress(&self.store_cache, PREFIX_LOCAL_ROOT_INDEX)
            .ok()
            .flatten()
            .is_some_and(|index| index >= state_root.index);
        if !cursor_is_committed {
            let mut cache = StoreCache::new_from_store(self.store.clone(), false);
            if stage_progress(&cache, PREFIX_LOCAL_ROOT_INDEX, state_root.index).is_err()
                || cache.try_commit().is_err()
            {
                return false;
            }
        }
        self.local_root_index
            .fetch_max(u64::from(state_root.index), Ordering::AcqRel);
        true
    }

    fn accept_validated_root(&self, state_root: StateRoot) -> bool {
        if state_root.version != CURRENT_VERSION || state_root.witness.is_none() {
            return false;
        }
        if let Some(expected) = self
            .reference_root_fetcher
            .lock()
            .as_ref()
            .and_then(|fetch| fetch(state_root.index))
            && expected != state_root.root_hash
        {
            warn!(
                target: "neo::state_service",
                index = state_root.index,
                expected = %expected,
                actual = %state_root.root_hash,
                "rejecting state root that differs from the configured reference"
            );
            return false;
        }

        let snapshot = StoreCache::new_from_store(self.store.clone(), true)
            .data_cache()
            .clone_cache();
        if !state_root.verify(&self.settings, &snapshot) {
            return false;
        }

        let key = state_root_record_key(state_root.index);
        let mut cache = StoreCache::new_from_store(self.store.clone(), false);
        let existing_item = cache.get(&key);
        if let Some(item) = &existing_item {
            let Some(existing) = deserialize_state_root_record(state_root.index, item.value())
            else {
                return false;
            };
            if existing.version != state_root.version || existing.root_hash != state_root.root_hash
            {
                return false;
            }
        }

        // Do not replace an unsigned local record merely to attach the network
        // witness. The validated cursor records quorum acceptance independently,
        // preserving both local and network progress across restart.
        if existing_item.is_none() {
            let item = StorageItem::from_bytes(serialize_state_root_record(&state_root));
            if cache.data_cache().try_add(key, item).is_err() {
                return false;
            }
        }
        if self.validated_root_index() < u64::from(state_root.index)
            && stage_progress(&cache, PREFIX_VALIDATED_ROOT_INDEX, state_root.index).is_err()
        {
            return false;
        }
        if cache.try_commit().is_err() {
            return false;
        }
        self.validated_root_index
            .fetch_max(u64::from(state_root.index), Ordering::AcqRel);
        true
    }
}

impl StateStoreService for StateRootStore {
    fn on_new_state_root(&self, state_root: StateRoot) -> bool {
        // Root records and monotonic progress share transactions. Serializing
        // writers prevents an older concurrent notification from overwriting a
        // newer cursor after the atomic in-memory fetch_max has advanced.
        let _guard = self.write_lock.lock();
        if state_root.witness.is_none() {
            self.accept_local_root(&state_root)
        } else {
            self.accept_validated_root(state_root)
        }
    }

    fn stage_local_root(&self, state_root: &StateRoot, snapshot: &DataCache) -> bool {
        let _guard = self.write_lock.lock();
        if !self.stage_local_root_progress(state_root, snapshot) {
            return false;
        }
        if state_root.index > self.local_root_index() as u32 {
            self.staged_local_roots
                .lock()
                .insert(state_root.index, state_root.root_hash);
        }
        true
    }

    fn on_local_root_committed(&self, state_root: &StateRoot) -> bool {
        if state_root.version != CURRENT_VERSION || state_root.witness.is_some() {
            return false;
        }
        let _guard = self.write_lock.lock();
        if let Some(expected) = self.staged_local_roots.lock().remove(&state_root.index) {
            if expected != state_root.root_hash {
                return false;
            }
            self.local_root_index
                .fetch_max(u64::from(state_root.index), Ordering::AcqRel);
            return true;
        }
        // Compatibility for direct embedders that do not use the canonical
        // precommit hook. This path retains the committed-record checks.
        self.accept_local_root(state_root)
    }

    fn discard_staged_local_root(&self, index: u32) {
        let _guard = self.write_lock.lock();
        self.staged_local_roots.lock().remove(&index);
    }

    fn local_root_index(&self) -> u64 {
        self.local_root_index.load(Ordering::Acquire)
    }

    fn validated_root_index(&self) -> u64 {
        self.validated_root_index.load(Ordering::Acquire)
    }

    fn set_reference_root_fetcher(
        &self,
        fetcher: Arc<dyn Fn(u32) -> Option<UInt256> + Send + Sync>,
    ) {
        *self.reference_root_fetcher.lock() = Some(fetcher);
    }

    fn flush(&self) {
        // Ingestion uses short-lived StoreCache transactions; there is no
        // additional pending write transaction owned by this service.
        self.store.flush();
    }
}

fn progress_key(prefix: u8) -> StorageKey {
    StorageKey::new(STATE_ROOT_STORAGE_ID, vec![prefix])
}

fn read_progress(cache: &StoreCache, prefix: u8) -> Result<Option<u32>, StateServiceError> {
    let Some(item) = cache.get(&progress_key(prefix)) else {
        return Ok(None);
    };
    let bytes: [u8; 8] = item.value().try_into().map_err(|_| {
        StateServiceError::new(format!(
            "malformed persisted state-root progress record 0x{prefix:02x}"
        ))
    })?;
    let index = u64::from_be_bytes(bytes);
    let index = u32::try_from(index).map_err(|_| {
        StateServiceError::new(format!(
            "persisted state-root progress record 0x{prefix:02x} exceeds u32 heights"
        ))
    })?;
    Ok(Some(index))
}

fn stage_progress(cache: &StoreCache, prefix: u8, index: u32) -> Result<(), String> {
    stage_progress_in_snapshot(cache.data_cache(), prefix, index)
}

fn stage_progress_in_snapshot(snapshot: &DataCache, prefix: u8, index: u32) -> Result<(), String> {
    let key = progress_key(prefix);
    let item = StorageItem::from_bytes(u64::from(index).to_be_bytes().to_vec());
    if snapshot.get(&key).is_some() {
        snapshot.try_update(key, item)
    } else {
        snapshot.try_add(key, item)
    }
    .map_err(|error| error.to_string())
}

fn recover_progress(
    cache: &StoreCache,
    progress_prefix: u8,
    validated: bool,
) -> Result<u32, StateServiceError> {
    if let Some(index) = read_progress(cache, progress_prefix)? {
        let record = read_state_root_record(cache, index)
            .map_err(StateServiceError::new)?
            .ok_or_else(|| {
                StateServiceError::new(format!(
                    "state-root progress 0x{progress_prefix:02x} references missing height {index}"
                ))
            })?;
        if record.version != CURRENT_VERSION {
            return Err(StateServiceError::new(format!(
                "state-root progress 0x{progress_prefix:02x} references unsupported version at height {index}"
            )));
        }
        return Ok(index);
    }

    // Backward-compatible recovery for records written before explicit
    // progress cursors existed. A signed record proves only validated progress;
    // an unsigned record proves only local progress.
    let mut recovered = 0;
    let prefix = StorageKey::new(STATE_ROOT_STORAGE_ID, vec![PREFIX_STATE_ROOT]);
    for (key, item) in cache.find(Some(&prefix), SeekDirection::Forward) {
        let raw = key.key();
        if raw.len() != 5 || raw[0] != PREFIX_STATE_ROOT {
            continue;
        }
        let index = u32::from_be_bytes([raw[1], raw[2], raw[3], raw[4]]);
        let record = deserialize_state_root_record(index, item.value()).ok_or_else(|| {
            StateServiceError::new(format!(
                "malformed persisted state-root record at height {index}"
            ))
        })?;
        if record.version != CURRENT_VERSION {
            return Err(StateServiceError::new(format!(
                "unsupported persisted state-root version at height {index}"
            )));
        }
        if record.witness.is_some() == validated {
            recovered = recovered.max(index);
        }
    }
    Ok(recovered)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state_service::test_support::serialize_role_public_keys;
    use neo_native_contracts::{NativeContract, Role, role_management::RoleManagement};
    use neo_protocol::witness::Witness;
    use neo_smart_contract::Contract;
    use neo_storage::error::{StorageError, StorageResult};
    use neo_storage::persistence::providers::memory_store::MemoryStore;
    use neo_storage::persistence::{
        ReadOnlyStore, ReadOnlyStoreGeneric, StoreSnapshot, WriteStore,
    };
    use neo_vm::OpCode;
    use neo_wallets::KeyPair;
    use std::any::Any;

    const HEIGHT: u32 = 5;

    struct Fixture {
        store: Arc<dyn Store>,
        cache: Arc<StoreCache>,
        settings: Arc<ProtocolSettings>,
        validator: KeyPair,
    }

    fn fixture() -> Fixture {
        let store: Arc<dyn Store> = Arc::new(MemoryStore::new());
        let cache = Arc::new(StoreCache::new(store.clone()));
        let validator = KeyPair::generate().expect("state validator keypair");
        let public_key = validator
            .get_public_key_point()
            .expect("state validator public key");
        let mut settings = ProtocolSettings::mainnet();
        settings.network = 0x4242_4242;
        settings.standby_committee = vec![public_key.clone()];
        settings.validators_count = 1;

        let mut designation = vec![Role::StateValidator as u8];
        designation.extend_from_slice(&HEIGHT.to_be_bytes());
        let mut writer = StoreCache::new(store.clone());
        writer
            .data_cache()
            .try_add(
                StorageKey::new(RoleManagement::new().id(), designation),
                StorageItem::from_bytes(serialize_role_public_keys(&[public_key])),
            )
            .expect("stage state validator designation");
        writer.try_commit().expect("commit state validators");

        Fixture {
            store,
            cache,
            settings: Arc::new(settings),
            validator,
        }
    }

    fn signed_root(fixture: &Fixture, index: u32, hash: UInt256) -> StateRoot {
        let mut root = StateRoot::new_current(index, hash);
        let root_hash = root.hash();
        let mut sign_data = Vec::with_capacity(36);
        sign_data.extend_from_slice(&fixture.settings.network.to_le_bytes());
        sign_data.extend_from_slice(&root_hash.to_array());
        let signature = fixture.validator.sign(&sign_data).expect("sign root");
        let mut invocation = vec![OpCode::PUSHDATA1.byte(), signature.len() as u8];
        invocation.extend_from_slice(&signature);
        root.witness = Some(Witness::new_with_scripts(
            invocation,
            Contract::create_multi_sig_redeem_script(
                1,
                &[fixture
                    .validator
                    .get_public_key_point()
                    .expect("state validator public key")],
            ),
        ));
        root
    }

    fn persist_root(store: &Arc<dyn Store>, root: &StateRoot) {
        let mut writer = StoreCache::new(store.clone());
        let key = state_root_record_key(root.index);
        let result = if writer.get(&key).is_some() {
            writer.data_cache().try_update(
                key,
                StorageItem::from_bytes(serialize_state_root_record(root)),
            )
        } else {
            writer.data_cache().try_add(
                key,
                StorageItem::from_bytes(serialize_state_root_record(root)),
            )
        };
        result.expect("stage state root");
        writer.try_commit().expect("commit state root");
    }

    fn reopen(fixture: &Fixture) -> StateRootStore {
        StateRootStore::new(
            fixture.store.clone(),
            Arc::new(StoreCache::new(fixture.store.clone())),
            fixture.settings.clone(),
        )
        .expect("reopen state root store")
    }

    #[test]
    fn local_root_requires_committed_record_and_recovers_cursor() {
        let fixture = fixture();
        let state_store = StateRootStore::new(
            fixture.store.clone(),
            fixture.cache.clone(),
            fixture.settings.clone(),
        )
        .unwrap();
        let root = StateRoot::new_current(HEIGHT, UInt256::from([1u8; 32]));

        assert!(!state_store.on_new_state_root(root.clone()));
        persist_root(&fixture.store, &root);
        assert!(state_store.on_new_state_root(root));
        assert_eq!(state_store.local_root_index(), u64::from(HEIGHT));
        assert_eq!(state_store.validated_root_index(), 0);

        let recovered = reopen(&fixture);
        assert_eq!(recovered.local_root_index(), u64::from(HEIGHT));
        assert_eq!(recovered.validated_root_index(), 0);
    }

    #[test]
    fn precommitted_local_cursor_is_consumed_without_losing_recovery() {
        let fixture = fixture();
        let state_store = StateRootStore::new(
            fixture.store.clone(),
            fixture.cache.clone(),
            fixture.settings.clone(),
        )
        .unwrap();
        let root = StateRoot::new_current(HEIGHT, UInt256::from([7u8; 32]));
        let mut writer = StoreCache::new(fixture.store.clone());
        writer
            .data_cache()
            .try_add(
                state_root_record_key(HEIGHT),
                StorageItem::from_bytes(serialize_state_root_record(&root)),
            )
            .unwrap();
        assert!(state_store.stage_local_root(&root, writer.data_cache()));
        writer.try_commit().unwrap();

        assert_eq!(state_store.local_root_index(), 0);
        assert!(state_store.on_local_root_committed(&root));
        assert_eq!(state_store.local_root_index(), u64::from(HEIGHT));
        assert_eq!(
            read_progress(&fixture.cache, PREFIX_LOCAL_ROOT_INDEX).unwrap(),
            Some(HEIGHT)
        );
        assert_eq!(reopen(&fixture).local_root_index(), u64::from(HEIGHT));
    }

    #[test]
    fn rollback_discards_staged_local_root_marker() {
        let fixture = fixture();
        let state_store = StateRootStore::new(
            fixture.store.clone(),
            fixture.cache.clone(),
            fixture.settings.clone(),
        )
        .unwrap();
        let root = StateRoot::new_current(HEIGHT, UInt256::from([8u8; 32]));
        let snapshot = DataCache::new(false);
        assert!(state_store.stage_local_root(&root, &snapshot));
        assert!(state_store.staged_local_roots.lock().contains_key(&HEIGHT));

        state_store.discard_staged_local_root(HEIGHT);
        assert!(state_store.staged_local_roots.lock().is_empty());
        assert!(!state_store.on_local_root_committed(&root));
        assert_eq!(state_store.local_root_index(), 0);
    }

    #[test]
    fn validated_root_before_local_recovers_independent_progress() {
        let fixture = fixture();
        let state_store = StateRootStore::new(
            fixture.store.clone(),
            fixture.cache.clone(),
            fixture.settings.clone(),
        )
        .unwrap();
        let root_hash = UInt256::from([2u8; 32]);
        let signed = signed_root(&fixture, HEIGHT, root_hash);
        assert!(state_store.on_new_state_root(signed));
        assert_eq!(state_store.local_root_index(), 0);
        assert_eq!(state_store.validated_root_index(), u64::from(HEIGHT));

        let recovered = reopen(&fixture);
        assert_eq!(recovered.local_root_index(), 0);
        assert_eq!(recovered.validated_root_index(), u64::from(HEIGHT));

        assert!(recovered.on_new_state_root(StateRoot::new_current(HEIGHT, root_hash)));
        let recovered_again = reopen(&fixture);
        assert_eq!(recovered_again.local_root_index(), u64::from(HEIGHT));
        assert_eq!(recovered_again.validated_root_index(), u64::from(HEIGHT));
    }

    #[test]
    fn validating_unsigned_local_record_preserves_it_and_recovers_both_cursors() {
        let fixture = fixture();
        let local = StateRoot::new_current(HEIGHT, UInt256::from([3u8; 32]));
        persist_root(&fixture.store, &local);
        let state_store = StateRootStore::new(
            fixture.store.clone(),
            fixture.cache.clone(),
            fixture.settings.clone(),
        )
        .unwrap();
        assert!(state_store.on_new_state_root(local.clone()));
        let signed = signed_root(&fixture, HEIGHT, local.root_hash);
        assert!(state_store.on_new_state_root(signed));

        let persisted = read_state_root_record(&fixture.cache, HEIGHT)
            .unwrap()
            .unwrap();
        assert!(persisted.witness.is_none());
        assert_eq!(persisted.root_hash, local.root_hash);

        let recovered = reopen(&fixture);
        assert_eq!(recovered.local_root_index(), u64::from(HEIGHT));
        assert_eq!(recovered.validated_root_index(), u64::from(HEIGHT));
    }

    #[test]
    fn reference_mismatch_and_invalid_witness_are_fail_closed() {
        let fixture = fixture();
        let state_store = StateRootStore::new(
            fixture.store.clone(),
            fixture.cache.clone(),
            fixture.settings.clone(),
        )
        .unwrap();
        let hash = UInt256::from([4u8; 32]);
        state_store.set_reference_root_fetcher(Arc::new(|_| Some(UInt256::from([9u8; 32]))));
        assert!(!state_store.on_new_state_root(signed_root(&fixture, HEIGHT, hash)));
        assert_eq!(state_store.validated_root_index(), 0);

        state_store.set_reference_root_fetcher(Arc::new(move |_| Some(hash)));
        let mut invalid = signed_root(&fixture, HEIGHT, hash);
        if let Some(byte) = invalid
            .witness
            .as_mut()
            .unwrap()
            .invocation_script
            .last_mut()
        {
            *byte ^= 1;
        }
        assert!(!state_store.on_new_state_root(invalid));
        assert_eq!(state_store.validated_root_index(), 0);
        assert!(
            read_state_root_record(&fixture.cache, HEIGHT)
                .unwrap()
                .is_none()
        );
    }

    #[test]
    fn malformed_or_orphaned_progress_fails_startup_closed() {
        let malformed_fixture = fixture();
        let mut writer = StoreCache::new(malformed_fixture.store.clone());
        writer
            .data_cache()
            .try_add(
                progress_key(PREFIX_VALIDATED_ROOT_INDEX),
                StorageItem::from_bytes(vec![0; 7]),
            )
            .unwrap();
        writer.try_commit().unwrap();
        assert!(
            StateRootStore::new(
                malformed_fixture.store.clone(),
                malformed_fixture.cache.clone(),
                malformed_fixture.settings.clone(),
            )
            .is_err()
        );

        let fixture = fixture();
        persist_root(
            &fixture.store,
            &StateRoot::new_current(HEIGHT, UInt256::from([5u8; 32])),
        );
        let mut writer = StoreCache::new(fixture.store.clone());
        writer
            .data_cache()
            .try_add(
                progress_key(PREFIX_VALIDATED_ROOT_INDEX),
                StorageItem::from_bytes(u64::from(HEIGHT + 1).to_be_bytes().to_vec()),
            )
            .unwrap();
        writer.try_commit().unwrap();
        assert!(
            StateRootStore::new(
                fixture.store.clone(),
                fixture.cache.clone(),
                fixture.settings.clone(),
            )
            .is_err()
        );
    }

    struct FailingStore {
        inner: MemoryStore,
    }

    impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for FailingStore {
        fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
            self.inner.try_get(key)
        }

        fn find(
            &self,
            prefix: Option<&StorageKey>,
            direction: SeekDirection,
        ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
            self.inner.find(prefix, direction)
        }
    }

    impl ReadOnlyStore for FailingStore {}

    impl WriteStore<Vec<u8>, Vec<u8>> for FailingStore {
        fn delete(&mut self, _key: Vec<u8>) -> StorageResult<()> {
            Err(injected_write_failure())
        }

        fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> StorageResult<()> {
            Err(injected_write_failure())
        }
    }

    impl Store for FailingStore {
        fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
            Arc::new(FailingSnapshot {
                inner: self.inner.snapshot(),
            })
        }

        fn on_new_snapshot(
            &self,
            _handler: neo_storage::persistence::store::OnNewSnapshotDelegate,
        ) {
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    struct FailingSnapshot {
        inner: Arc<dyn StoreSnapshot>,
    }

    impl ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>> for FailingSnapshot {
        fn try_get(&self, key: &Vec<u8>) -> Option<Vec<u8>> {
            self.inner.try_get(key)
        }

        fn find(
            &self,
            prefix: Option<&Vec<u8>>,
            direction: SeekDirection,
        ) -> Box<dyn Iterator<Item = (Vec<u8>, Vec<u8>)> + '_> {
            self.inner.find(prefix, direction)
        }
    }

    impl WriteStore<Vec<u8>, Vec<u8>> for FailingSnapshot {
        fn delete(&mut self, _key: Vec<u8>) -> StorageResult<()> {
            Err(injected_write_failure())
        }

        fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> StorageResult<()> {
            Err(injected_write_failure())
        }
    }

    impl StoreSnapshot for FailingSnapshot {
        fn store(&self) -> Arc<dyn Store> {
            self.inner.store()
        }

        fn try_commit(&mut self) -> StorageResult<()> {
            Err(injected_write_failure())
        }
    }

    fn injected_write_failure() -> StorageError {
        StorageError::InvalidOperation {
            message: "injected state-root write failure".to_string(),
        }
    }

    #[test]
    fn commit_failure_does_not_advance_validated_progress_or_persist_root() {
        let mut fixture = fixture();
        let inner = MemoryStore::new();
        let mut designation = vec![Role::StateValidator as u8];
        designation.extend_from_slice(&HEIGHT.to_be_bytes());
        let public_key = fixture
            .validator
            .get_public_key_point()
            .expect("state validator public key");
        let mut writer = StoreCache::new(Arc::new(inner.clone()));
        writer
            .data_cache()
            .try_add(
                StorageKey::new(RoleManagement::new().id(), designation),
                StorageItem::from_bytes(serialize_role_public_keys(&[public_key])),
            )
            .unwrap();
        writer.try_commit().unwrap();

        fixture.store = Arc::new(FailingStore { inner });
        fixture.cache = Arc::new(StoreCache::new(fixture.store.clone()));
        let state_store = StateRootStore::new(
            fixture.store.clone(),
            fixture.cache.clone(),
            fixture.settings.clone(),
        )
        .unwrap();
        assert!(!state_store.on_new_state_root(signed_root(
            &fixture,
            HEIGHT,
            UInt256::from([6u8; 32]),
        )));
        assert_eq!(state_store.validated_root_index(), 0);
        assert!(fixture.cache.get(&state_root_record_key(HEIGHT)).is_none());
        assert!(
            fixture
                .cache
                .get(&progress_key(PREFIX_VALIDATED_ROOT_INDEX))
                .is_none()
        );
    }
}
