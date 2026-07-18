//! Cold-first authoritative pack manager for exact StateService MPT nodes.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread::{self, JoinHandle};

use anyhow::{Context, ensure};
use neo_state_packs::authority::{AUTHORITATIVE_HIGH_WATER_KEY, AuthoritativeHighWaterRecord};
use neo_state_packs::{
    PACK_FRAME_FORMAT_VERSION, PACK_INDEX_FORMAT_VERSION, PACK_KEY_BYTES,
    PACK_MANIFEST_FORMAT_VERSION, PackOpKind, PackOperation, PackStore, PackStoreOptions,
    Snapshot as PackSnapshot,
};
use neo_state_service::mpt_store::{PreparedMptCommit, PreparedMptMetadataOverlay};
use neo_state_service::{MptNodeReadGeneration, MptNodeReadSnapshot, MptNodeSnapshotFactory};
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::{CoordinatedCommitMarker, RawReadOnlyStore, TransactionalStore};
use neo_storage::{StorageError, StorageResult};
use parking_lot::{Condvar, Mutex, RwLock};
use serde::Deserialize;
use tracing::{error, warn};

const CHECKPOINT_SCHEMA_VERSION: u32 = 2;
const CURRENT_LOCAL_ROOT_INDEX: &[u8] = &[0x02];
const STATE_ROOT_PREFIX: u8 = 0x01;
const STATE_ROOT_VALUE_ROOT_OFFSET: usize = 5;
const STATE_ROOT_VALUE_UNSIGNED_LEN: usize = 1 + 4 + 32;

#[derive(Debug, Deserialize)]
struct CheckpointMarker {
    schema_version: u32,
    authoritative_ready: bool,
    complete: bool,
    network_magic: String,
    source_height: u32,
    source_root_internal_bytes: String,
    source_namespace_sha256: String,
    pack_frame_format_version: u32,
    pack_index_format_version: u32,
    pack_manifest_format_version: u32,
    tip_epoch: u64,
    tip_frame_end: u64,
    tip_payload_sha256: String,
}

struct PublishedGeneration {
    sequence: u64,
    snapshot: Arc<PackNodeSnapshot>,
    marker: AuthoritativeHighWaterRecord,
}

struct PackNodeSnapshot {
    inner: Arc<PackSnapshot>,
}

#[derive(Default)]
struct MaintenanceState {
    progress: u64,
    failure: Option<String>,
}

/// Coalescing single-worker scheduler for derived index maintenance.
///
/// The worker builds immutable merge output outside the pack writer lock and
/// holds that lock only for source validation, manifest publication, and the
/// semantically equivalent read-snapshot swap.
struct PackMaintenance {
    request: Option<SyncSender<()>>,
    state: Arc<(Mutex<MaintenanceState>, Condvar)>,
    handle: Option<JoinHandle<()>>,
}

impl PackMaintenance {
    fn spawn(
        writer: Arc<Mutex<Option<PackStore>>>,
        publication: Arc<RwLock<PublishedGeneration>>,
        path: PathBuf,
    ) -> anyhow::Result<Self> {
        let (request, receiver) = sync_channel(1);
        let state = Arc::new((Mutex::new(MaintenanceState::default()), Condvar::new()));
        let worker_state = Arc::clone(&state);
        let worker_writer = Arc::clone(&writer);
        let handle = thread::Builder::new()
            .name("neo-pack-compact".to_owned())
            .spawn(move || {
                if let Err(error) =
                    run_pack_maintenance(receiver, &worker_writer, &publication, &worker_state)
                {
                    let message = format!("{error:#}");
                    error!(
                        target: "neo::state_packs",
                        path = %path.display(),
                        error = %message,
                        "authoritative derived-index worker failed; poisoning writer until restart"
                    );
                    *worker_writer.lock() = None;
                    let (state, wake) = &*worker_state;
                    let mut state = state.lock();
                    state.failure = Some(message);
                    state.progress = state.progress.wrapping_add(1);
                    wake.notify_all();
                }
            })
            .context("spawn authoritative pack compaction worker")?;
        let maintenance = Self {
            request: Some(request),
            state,
            handle: Some(handle),
        };
        maintenance
            .request()
            .map_err(|error| anyhow::anyhow!(error.to_string()))?;
        Ok(maintenance)
    }

    fn request(&self) -> StorageResult<()> {
        let sender = self.request.as_ref().ok_or_else(|| {
            StorageError::backend("authoritative pack maintenance is shutting down")
        })?;
        match sender.try_send(()) {
            Ok(()) | Err(TrySendError::Full(())) => Ok(()),
            Err(TrySendError::Disconnected(())) => Err(StorageError::backend(
                "authoritative pack maintenance worker disconnected",
            )),
        }
    }

    fn progress(&self) -> u64 {
        self.state.0.lock().progress
    }

    fn ensure_healthy(&self) -> StorageResult<()> {
        let state = self.state.0.lock();
        if let Some(error) = &state.failure {
            return Err(StorageError::backend(format!(
                "authoritative pack maintenance failed: {error}"
            )));
        }
        Ok(())
    }

    fn wait_for_progress(&self, observed: u64) -> StorageResult<()> {
        let (state, wake) = &*self.state;
        let mut state = state.lock();
        while state.progress == observed && state.failure.is_none() {
            wake.wait(&mut state);
        }
        if let Some(error) = &state.failure {
            return Err(StorageError::backend(format!(
                "authoritative pack maintenance failed: {error}"
            )));
        }
        Ok(())
    }
}

impl Drop for PackMaintenance {
    fn drop(&mut self) {
        self.request.take();
        if let Some(handle) = self.handle.take() {
            let _ = handle.join();
        }
    }
}

fn run_pack_maintenance(
    receiver: Receiver<()>,
    writer: &Mutex<Option<PackStore>>,
    publication: &RwLock<PublishedGeneration>,
    state: &(Mutex<MaintenanceState>, Condvar),
) -> anyhow::Result<()> {
    while receiver.recv().is_ok() {
        loop {
            let plan = {
                let writer = writer.lock();
                let store = writer
                    .as_ref()
                    .context("authoritative pack writer is unavailable")?;
                store.plan_compaction()?
            };
            let Some(plan) = plan else {
                break;
            };
            let prepared = plan.build()?;
            {
                let mut writer = writer.lock();
                let store = writer
                    .as_mut()
                    .context("authoritative pack writer is unavailable")?;
                store.adopt_compaction(prepared)?;
                let snapshot = Arc::new(PackNodeSnapshot {
                    inner: Arc::new(store.snapshot()?),
                });
                // Lock ordering matches canonical commit: writer, publication.
                publication.write().snapshot = snapshot;
            }
            note_maintenance_progress(state);
            thread::yield_now();
        }
        note_maintenance_progress(state);
    }
    Ok(())
}

fn note_maintenance_progress(state: &(Mutex<MaintenanceState>, Condvar)) {
    let (state, wake) = state;
    let mut state = state.lock();
    state.progress = state.progress.wrapping_add(1);
    wake.notify_all();
}

impl MptNodeReadSnapshot for PackNodeSnapshot {
    fn try_get_node_bytes(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
        let key = exact_node_key(key)?;
        self.inner
            .get(&key)
            .map_err(|error| StorageError::backend(format!("authoritative pack read: {error:#}")))
    }

    fn try_get_node_bytes_sorted(&self, keys: &[&[u8]]) -> StorageResult<Vec<Option<Vec<u8>>>> {
        let keys = keys
            .iter()
            .map(|key| exact_node_key(key))
            .collect::<StorageResult<Vec<_>>>()?;
        self.inner.get_many_sorted(&keys).map_err(|error| {
            StorageError::backend(format!("authoritative pack sorted read: {error:#}"))
        })
    }
}

/// Shared writer, publication barrier, and immutable read-generation factory.
pub(in crate::node) struct AuthoritativeNodePack {
    network_magic: u32,
    store_identity: [u8; 32],
    path: PathBuf,
    max_index_memory_bytes: u64,
    maintenance: PackMaintenance,
    writer: Arc<Mutex<Option<PackStore>>>,
    publication: Arc<RwLock<PublishedGeneration>>,
}

impl AuthoritativeNodePack {
    /// Opens a complete checkpoint and binds it to the canonical MDBX marker
    /// and StateService metadata. Every mismatch fails startup.
    pub(in crate::node) fn open(
        path: &Path,
        max_index_memory_bytes: u64,
        network_magic: u32,
        state_backing: &RuntimeStore,
    ) -> anyhow::Result<Arc<Self>> {
        Self::open_with_options(
            path,
            max_index_memory_bytes,
            network_magic,
            state_backing,
            PackStoreOptions::default(),
        )
    }

    /// Opens authoritative packs with a physically separate random-advised
    /// mmap for exact point reads.
    pub(in crate::node) fn open_with_random_point_mmap(
        path: &Path,
        max_index_memory_bytes: u64,
        network_magic: u32,
        state_backing: &RuntimeStore,
    ) -> anyhow::Result<Arc<Self>> {
        Self::open_with_options(
            path,
            max_index_memory_bytes,
            network_magic,
            state_backing,
            PackStoreOptions {
                random_point_mmap: true,
            },
        )
    }

    fn open_with_options(
        path: &Path,
        max_index_memory_bytes: u64,
        network_magic: u32,
        state_backing: &RuntimeStore,
        options: PackStoreOptions,
    ) -> anyhow::Result<Arc<Self>> {
        let checkpoint = read_checkpoint(path)?;
        validate_checkpoint(&checkpoint, network_magic)?;
        let store_identity = decode_hash(
            &checkpoint.source_namespace_sha256,
            "checkpoint source namespace digest",
        )?;
        let checkpoint_root = decode_hash(
            &checkpoint.source_root_internal_bytes,
            "checkpoint source root",
        )?;
        let checkpoint_payload =
            decode_hash(&checkpoint.tip_payload_sha256, "checkpoint tip checksum")?;
        let state_tip = read_state_tip(state_backing)?;
        let durable_marker = state_backing
            .maintenance_metadata(AUTHORITATIVE_HIGH_WATER_KEY)
            .context("read authoritative pack high-water marker")?;

        let (store, marker) = match durable_marker {
            Some(bytes) => {
                let marker = AuthoritativeHighWaterRecord::decode(&bytes)
                    .context("decode authoritative pack high-water marker")?;
                marker
                    .validate_identity(network_magic, store_identity)
                    .context("validate authoritative pack marker identity")?;
                ensure!(
                    (marker.block_index, marker.state_root) == state_tip,
                    "authoritative pack marker tip ({}, 0x{}) differs from StateService metadata ({}, 0x{})",
                    marker.block_index,
                    hex::encode(marker.state_root),
                    state_tip.0,
                    hex::encode(state_tip.1)
                );
                ensure!(
                    marker.block_index >= checkpoint.source_height,
                    "authoritative marker predates its base checkpoint"
                );
                let store = PackStore::open_at_commit_horizon_with_options(
                    path,
                    max_index_memory_bytes,
                    Some(marker.commit_horizon()),
                    options,
                )
                .with_context(|| {
                    format!("open authoritative pack at MDBX marker {}", path.display())
                })?;
                (store, marker)
            }
            None => {
                ensure!(
                    state_tip == (checkpoint.source_height, checkpoint_root),
                    "StateService tip does not equal the unactivated checkpoint base"
                );
                let checkpoint_horizon = neo_state_packs::PackCommitHorizon {
                    epoch: checkpoint.tip_epoch,
                    payload_sha256: checkpoint_payload,
                };
                let store = PackStore::open_at_commit_horizon_with_options(
                    path,
                    max_index_memory_bytes,
                    Some(checkpoint_horizon),
                    options,
                )
                .with_context(|| format!("open authoritative checkpoint {}", path.display()))?;
                let receipt = store
                    .last_frame_receipt()
                    .context("authoritative checkpoint has no pack tip")?;
                ensure!(
                    receipt.epoch == checkpoint.tip_epoch
                        && receipt.frame_end == checkpoint.tip_frame_end
                        && receipt.payload_sha256 == checkpoint_payload,
                    "authoritative checkpoint tip differs from checkpoint.json"
                );
                let marker = AuthoritativeHighWaterRecord::new(
                    network_magic,
                    store_identity,
                    receipt,
                    checkpoint.source_height,
                    checkpoint_root,
                );
                (store, marker)
            }
        };
        let pinned = Arc::new(
            store
                .snapshot()
                .context("pin initial authoritative pack view")?,
        );
        let mut root_key = [0u8; PACK_KEY_BYTES];
        root_key[0] = 0xF0;
        root_key[1..].copy_from_slice(&state_tip.1);
        ensure!(
            pinned
                .get(&root_key)
                .context("resolve authoritative StateService root node")?
                .is_some(),
            "authoritative pack does not contain the StateService root node"
        );
        let snapshot = Arc::new(PackNodeSnapshot { inner: pinned });
        let writer = Arc::new(Mutex::new(Some(store)));
        let publication = Arc::new(RwLock::new(PublishedGeneration {
            sequence: 0,
            snapshot,
            marker,
        }));
        let maintenance = PackMaintenance::spawn(
            Arc::clone(&writer),
            Arc::clone(&publication),
            path.to_path_buf(),
        )?;
        Ok(Arc::new(Self {
            network_magic,
            store_identity,
            path: path.to_path_buf(),
            max_index_memory_bytes,
            maintenance,
            writer,
            publication,
        }))
    }

    /// Seals node bytes, commits metadata plus marker through `commit`, then
    /// performs the only post-marker operation: an infallible Arc swap.
    pub(in crate::node) fn commit_prepared<F>(
        &self,
        prepared: &mut PreparedMptCommit,
        commit: F,
    ) -> StorageResult<()>
    where
        F: FnOnce(
            &mut PreparedMptMetadataOverlay<'_>,
            &CoordinatedCommitMarker,
        ) -> StorageResult<()>,
    {
        prepared.materialize_deferred_node_overlay()?;
        let expected_operations = prepared.materialized_node_operation_count();
        let mut operations = Vec::with_capacity(expected_operations);
        let mut conversion_error = None;
        prepared.visit_materialized_node_overlay(&mut |key: &[u8], value: Option<&[u8]>| {
            if conversion_error.is_some() {
                return;
            }
            match exact_node_key(key) {
                Ok(key) => operations.push(PackOperation {
                    key,
                    kind: value.map_or(PackOpKind::Tombstone, |value| {
                        PackOpKind::Put(value.to_vec())
                    }),
                }),
                Err(error) => conversion_error = Some(error),
            }
        });
        if let Some(error) = conversion_error {
            return Err(error);
        }
        if operations.len() != expected_operations {
            return Err(StorageError::invalid_operation(
                "authoritative node overlay conversion omitted an operation",
            ));
        }

        let mut writer = loop {
            self.maintenance.ensure_healthy()?;
            let observed_progress = self.maintenance.progress();
            let writer = self.writer.lock();
            let store = writer.as_ref().ok_or_else(|| {
                StorageError::backend(
                    "authoritative pack writer is poisoned; restart for marker recovery",
                )
            })?;
            if operations.is_empty() || !store.compaction_debt().backpressure_required {
                break writer;
            }
            drop(writer);
            self.maintenance.request()?;
            self.maintenance.wait_for_progress(observed_progress)?;
        };
        let store = writer.as_mut().ok_or_else(|| {
            StorageError::backend(
                "authoritative pack writer is poisoned; restart for marker recovery",
            )
        })?;
        let base_marker = self.publication.read().marker;
        let block_index = prepared.block_index();
        let state_root = prepared.root_hash().to_array();
        let sealed = if operations.is_empty() {
            None
        } else {
            let pending = match store.prepare_append(&operations) {
                Ok(pending) => pending,
                Err(error) => {
                    *writer = None;
                    return Err(StorageError::backend(format!(
                        "authoritative pack prepare failed: {error:#}"
                    )));
                }
            };
            let receipt = pending.receipt();
            let sealed = match store.seal_prepared(pending) {
                Ok(sealed) => sealed,
                Err(error) => {
                    *writer = None;
                    return Err(StorageError::backend(format!(
                        "authoritative pack seal failed: {error:#}"
                    )));
                }
            };
            let next_snapshot = Arc::new(PackNodeSnapshot {
                inner: sealed.into_snapshot(),
            });
            Some((receipt, next_snapshot))
        };
        let next_marker = sealed.as_ref().map_or_else(
            || base_marker.with_state_tip(block_index, state_root),
            |(receipt, _)| {
                AuthoritativeHighWaterRecord::new(
                    self.network_magic,
                    self.store_identity,
                    *receipt,
                    block_index,
                    state_root,
                )
            },
        );
        let marker = CoordinatedCommitMarker {
            key: AUTHORITATIVE_HIGH_WATER_KEY.to_vec(),
            value: next_marker.encode().to_vec(),
        };

        let mut publication = self.publication.write();
        if publication.marker != base_marker {
            *writer = None;
            return Err(StorageError::backend(
                "authoritative pack generation changed during serialized publication",
            ));
        }
        let mut metadata = prepared.metadata_overlay_source();
        if let Err(error) = commit(&mut metadata, &marker) {
            if sealed.is_some() {
                *writer = None;
            }
            return Err(error);
        }
        if let Some((_, next_snapshot)) = sealed {
            publication.sequence = publication.sequence.wrapping_add(1);
            publication.snapshot = next_snapshot;
        }
        publication.marker = next_marker;
        let maintenance_needed = !operations.is_empty() && store.compaction_debt().excess_runs > 0;
        drop(publication);
        drop(writer);

        if maintenance_needed && let Err(error) = self.maintenance.request() {
            warn!(
                target: "neo::state_packs",
                path = %self.path.display(),
                max_index_memory_bytes = self.max_index_memory_bytes,
                error = %error,
                "authoritative marker committed but derived pack maintenance could not be scheduled; poisoning writer until restart"
            );
            *self.writer.lock() = None;
        }
        Ok(())
    }
}

impl MptNodeSnapshotFactory for AuthoritativeNodePack {
    fn snapshot(&self) -> Arc<dyn MptNodeReadSnapshot> {
        self.publication.read().snapshot.clone()
    }

    fn pinned_generation(&self) -> MptNodeReadGeneration {
        let publication = self.publication.read();
        MptNodeReadGeneration::new(publication.sequence, publication.snapshot.clone())
    }

    fn is_generation_current(&self, sequence: u64) -> bool {
        self.publication.read().sequence == sequence
    }
}

fn exact_node_key(key: &[u8]) -> StorageResult<[u8; PACK_KEY_BYTES]> {
    if key.len() != PACK_KEY_BYTES || key.first() != Some(&0xF0) {
        return Err(StorageError::invalid_operation(
            "authoritative pack received a key outside the exact MPT node namespace",
        ));
    }
    Ok(key.try_into().expect("validated pack key length"))
}

fn read_checkpoint(path: &Path) -> anyhow::Result<CheckpointMarker> {
    let marker_path = path.join("checkpoint.json");
    let bytes = fs::read(&marker_path)
        .with_context(|| format!("read checkpoint marker {}", marker_path.display()))?;
    serde_json::from_slice(&bytes)
        .with_context(|| format!("decode checkpoint marker {}", marker_path.display()))
}

fn validate_checkpoint(marker: &CheckpointMarker, network_magic: u32) -> anyhow::Result<()> {
    ensure!(
        marker.schema_version == CHECKPOINT_SCHEMA_VERSION,
        "checkpoint schema {} is unsupported",
        marker.schema_version
    );
    ensure!(
        marker.complete && marker.authoritative_ready,
        "checkpoint is not complete and explicitly authoritative-ready"
    );
    ensure!(
        parse_network_magic(&marker.network_magic)? == network_magic,
        "checkpoint network differs from configured network"
    );
    ensure!(
        (
            marker.pack_frame_format_version,
            marker.pack_index_format_version,
            marker.pack_manifest_format_version,
        ) == (
            PACK_FRAME_FORMAT_VERSION,
            PACK_INDEX_FORMAT_VERSION,
            PACK_MANIFEST_FORMAT_VERSION,
        ),
        "checkpoint pack format tuple differs from this binary"
    );
    ensure!(marker.tip_frame_end > 0, "checkpoint pack tip is empty");
    Ok(())
}

fn read_state_tip(store: &RuntimeStore) -> anyhow::Result<(u32, [u8; 32])> {
    let index_bytes = store
        .try_get_bytes_result(CURRENT_LOCAL_ROOT_INDEX)
        .context("read StateService current local root index")?
        .context("StateService current local root index is absent")?;
    let index = u32::from_le_bytes(
        index_bytes
            .as_slice()
            .try_into()
            .context("StateService current local root index is not four bytes")?,
    );
    let mut key = Vec::with_capacity(5);
    key.push(STATE_ROOT_PREFIX);
    key.extend_from_slice(&index.to_be_bytes());
    let value = store
        .try_get_bytes_result(&key)
        .context("read StateService current root record")?
        .context("StateService current root record is absent")?;
    ensure!(
        value.len() >= STATE_ROOT_VALUE_UNSIGNED_LEN,
        "StateService current root record is truncated"
    );
    ensure!(
        u32::from_le_bytes(value[1..5].try_into().expect("four-byte root index")) == index,
        "StateService root record index does not match its key"
    );
    Ok((
        index,
        value[STATE_ROOT_VALUE_ROOT_OFFSET..STATE_ROOT_VALUE_UNSIGNED_LEN]
            .try_into()
            .expect("fixed StateService root range"),
    ))
}

fn parse_network_magic(value: &str) -> anyhow::Result<u32> {
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .context("checkpoint network magic lacks 0x prefix")?;
    u32::from_str_radix(value, 16).context("decode checkpoint network magic")
}

fn decode_hash(value: &str, field: &'static str) -> anyhow::Result<[u8; 32]> {
    let value = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .with_context(|| format!("{field} lacks 0x prefix"))?;
    let bytes = hex::decode(value).with_context(|| format!("decode {field}"))?;
    bytes
        .try_into()
        .map_err(|bytes: Vec<u8>| anyhow::anyhow!("{field} has {} bytes, expected 32", bytes.len()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_state_packs::{PackOpKind, PackOperation};
    use neo_storage::mdbx::MdbxStoreProvider;
    use neo_storage::persistence::providers::MemoryStore;
    use neo_storage::persistence::storage::StorageConfig;
    use neo_storage::persistence::{
        RawOverlaySink, RawOverlaySource, ReadOnlyStoreGeneric, SeekDirection, WriteStore,
    };
    use neo_storage::{DataCache, StorageItem, StorageKey};
    use serde_json::json;
    use std::collections::BTreeSet;
    use tempfile::tempdir;

    struct Fixture {
        _temporary: tempfile::TempDir,
        pack_path: PathBuf,
        canonical: RuntimeStore,
        state: RuntimeStore,
        oracle: Arc<MemoryStore>,
        root: [u8; 32],
    }

    fn fixture() -> Fixture {
        let temporary = tempdir().expect("temporary authority fixture");
        let pack_path = temporary.path().join("packs");
        let source_backing = Arc::new(MemoryStore::new());
        let source =
            neo_state_service::MptStore::from_memory_store(Arc::clone(&source_backing), true)
                .expect("open source MPT");
        let mut storage_key = 5i32.to_le_bytes().to_vec();
        storage_key.push(0xAA);
        let root = source
            .apply_block_changes(
                0,
                None,
                &[neo_state_service::MptChange::Put {
                    key: storage_key,
                    value: b"genesis-value".to_vec(),
                }],
            )
            .expect("build source MPT")
            .to_array();
        let source_entries = <MemoryStore as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::find(
            source_backing.as_ref(),
            None,
            SeekDirection::Forward,
        )
        .collect::<Vec<_>>();
        let node_operations = source_entries
            .iter()
            .filter(|(key, _)| key.len() == PACK_KEY_BYTES && key[0] == 0xF0)
            .map(|(key, value)| PackOperation {
                key: key.as_slice().try_into().expect("exact source node key"),
                kind: PackOpKind::Put(value.clone()),
            })
            .collect::<Vec<_>>();
        assert!(!node_operations.is_empty());
        let mut pack = PackStore::create(&pack_path, 64 * 1024 * 1024).expect("create pack");
        pack.append(&node_operations).expect("append source nodes");
        let receipt = pack.last_frame_receipt().expect("pack receipt");
        drop(pack);
        let checkpoint = json!({
            "schema_version": CHECKPOINT_SCHEMA_VERSION,
            "authoritative_ready": true,
            "complete": true,
            "network_magic": "0x334F454E",
            "source_height": 0,
            "source_root_internal_bytes": format!("0x{}", hex::encode(root)),
            "source_namespace_sha256": format!("0x{}", hex::encode([0x11; 32])),
            "pack_frame_format_version": PACK_FRAME_FORMAT_VERSION,
            "pack_index_format_version": PACK_INDEX_FORMAT_VERSION,
            "pack_manifest_format_version": PACK_MANIFEST_FORMAT_VERSION,
            "tip_epoch": receipt.epoch,
            "tip_frame_end": receipt.frame_end,
            "tip_payload_sha256": format!("0x{}", hex::encode(receipt.payload_sha256)),
        });
        fs::write(
            pack_path.join("checkpoint.json"),
            serde_json::to_vec_pretty(&checkpoint).expect("encode checkpoint"),
        )
        .expect("write checkpoint");

        let canonical_mdbx = MdbxStoreProvider::new(StorageConfig {
            path: temporary.path().join("canonical"),
            ..Default::default()
        })
        .get_mdbx_store(std::path::Path::new(""))
        .expect("open canonical MDBX");
        let canonical = RuntimeStore::Mdbx(canonical_mdbx.clone());
        let mut state = RuntimeStore::Mdbx(
            canonical_mdbx
                .open_named_table(neo_state_service::MDBX_STATE_SERVICE_NAMESPACE)
                .expect("open state table"),
        );
        for (key, value) in source_entries
            .into_iter()
            .filter(|(key, _)| !(key.len() == PACK_KEY_BYTES && key[0] == 0xF0))
        {
            state.put(key, value).expect("copy StateService metadata");
        }
        Fixture {
            _temporary: temporary,
            pack_path,
            canonical,
            state,
            oracle: source_backing,
            root,
        }
    }

    fn exact_oracle_node_entries(oracle: &MemoryStore) -> Vec<(Vec<u8>, Vec<u8>)> {
        <MemoryStore as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::find(
            oracle,
            None,
            SeekDirection::Forward,
        )
        .filter(|(key, _)| key.len() == PACK_KEY_BYTES && key[0] == 0xF0)
        .collect()
    }

    fn copy_memory_store(source: &MemoryStore) -> Arc<MemoryStore> {
        let mut copy = MemoryStore::new();
        for (key, value) in <MemoryStore as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::find(
            source,
            None,
            SeekDirection::Forward,
        ) {
            copy.put(key, value).expect("copy oracle row");
        }
        Arc::new(copy)
    }

    fn assert_pack_matches_oracle(
        authority: &AuthoritativeNodePack,
        oracle: &MemoryStore,
        observed_node_keys: &mut BTreeSet<Vec<u8>>,
    ) {
        observed_node_keys.extend(
            exact_oracle_node_entries(oracle)
                .into_iter()
                .map(|(key, _)| key),
        );
        assert!(!observed_node_keys.is_empty());
        let snapshot = authority.snapshot();
        for key in observed_node_keys.iter() {
            assert_eq!(
                snapshot
                    .try_get_node_bytes(key)
                    .expect("read authoritative node"),
                oracle.try_get_bytes(key),
                "authoritative node bytes differ for 0x{}",
                hex::encode(key)
            );
        }
    }

    fn assert_mdbx_node_namespace_empty(state: &RuntimeStore) {
        let prefix = vec![0xF0];
        assert!(
            <RuntimeStore as ReadOnlyStoreGeneric<Vec<u8>, Vec<u8>>>::find(
                state,
                Some(&prefix),
                SeekDirection::Forward,
            )
            .next()
            .is_none(),
            "authoritative node rows must never enter MDBX"
        );
    }

    struct RecordingNodeSnapshot {
        sequence: u64,
        inner: Arc<dyn MptNodeReadSnapshot>,
        reads: Arc<Mutex<Vec<(u64, Vec<u8>)>>>,
    }

    impl MptNodeReadSnapshot for RecordingNodeSnapshot {
        fn try_get_node_bytes(&self, key: &[u8]) -> StorageResult<Option<Vec<u8>>> {
            self.reads.lock().push((self.sequence, key.to_vec()));
            self.inner.try_get_node_bytes(key)
        }

        fn try_get_node_bytes_sorted(&self, keys: &[&[u8]]) -> StorageResult<Vec<Option<Vec<u8>>>> {
            self.reads
                .lock()
                .extend(keys.iter().map(|key| (self.sequence, key.to_vec())));
            self.inner.try_get_node_bytes_sorted(keys)
        }
    }

    struct RecordingNodeFactory {
        authority: Arc<AuthoritativeNodePack>,
        reads: Arc<Mutex<Vec<(u64, Vec<u8>)>>>,
    }

    impl RecordingNodeFactory {
        fn new(authority: Arc<AuthoritativeNodePack>) -> Arc<Self> {
            Arc::new(Self {
                authority,
                reads: Arc::new(Mutex::new(Vec::new())),
            })
        }

        fn clear_reads(&self) {
            self.reads.lock().clear();
        }

        fn read_key_in_generation(&self, sequence: u64, key: &[u8]) -> bool {
            self.reads
                .lock()
                .iter()
                .any(|(read_sequence, read_key)| *read_sequence == sequence && read_key == key)
        }

        fn wrap_generation(&self, generation: MptNodeReadGeneration) -> MptNodeReadGeneration {
            let sequence = generation.sequence();
            MptNodeReadGeneration::new(
                sequence,
                Arc::new(RecordingNodeSnapshot {
                    sequence,
                    inner: generation.snapshot(),
                    reads: Arc::clone(&self.reads),
                }),
            )
        }
    }

    impl MptNodeSnapshotFactory for RecordingNodeFactory {
        fn snapshot(&self) -> Arc<dyn MptNodeReadSnapshot> {
            self.wrap_generation(self.authority.pinned_generation())
                .snapshot()
        }

        fn pinned_generation(&self) -> MptNodeReadGeneration {
            self.wrap_generation(self.authority.pinned_generation())
        }

        fn is_generation_current(&self, sequence: u64) -> bool {
            self.authority.is_generation_current(sequence)
        }
    }

    #[test]
    fn checkpoint_open_is_fail_closed_and_serves_the_bound_root() {
        let fixture = fixture();
        let authority = AuthoritativeNodePack::open_with_random_point_mmap(
            &fixture.pack_path,
            64 * 1024 * 1024,
            0x334F_454E,
            &fixture.state,
        )
        .expect("open authority");
        let mut key = [0u8; PACK_KEY_BYTES];
        key[0] = 0xF0;
        key[1..].copy_from_slice(&fixture.root);
        assert!(
            authority
                .snapshot()
                .try_get_node_bytes(&key)
                .expect("read root")
                .is_some()
        );
        drop(authority);

        let error = AuthoritativeNodePack::open(
            &fixture.pack_path,
            64 * 1024 * 1024,
            0xDEAD_BEEF,
            &fixture.state,
        )
        .err()
        .expect("wrong network must fail");
        assert!(error.to_string().contains("network"), "{error:#}");
    }

    struct TestOverlay(Vec<(Vec<u8>, Option<Vec<u8>>)>);

    impl RawOverlaySource for TestOverlay {
        fn visit_raw_overlay<S>(&mut self, sink: &mut S)
        where
            S: RawOverlaySink + ?Sized,
        {
            self.0.sort_unstable_by(|left, right| left.0.cmp(&right.0));
            for (key, value) in &self.0 {
                sink.visit(key, value.as_deref());
            }
        }
    }

    #[test]
    fn authoritative_commit_writes_nodes_only_to_pack_and_reopens_from_marker() {
        let fixture = fixture();
        let authority = AuthoritativeNodePack::open(
            &fixture.pack_path,
            64 * 1024 * 1024,
            0x334F_454E,
            &fixture.state,
        )
        .expect("open authority");
        let backing = Arc::new(fixture.state.clone());
        let factory: Arc<dyn MptNodeSnapshotFactory> = authority.clone();
        let state_store = Arc::new(
            neo_state_service::StateStore::with_mpt_store_and_node_snapshots(
                true, backing, factory,
            )
            .expect("open split StateService"),
        );
        let handlers =
            neo_state_service::commit_handlers::StateServiceCommitHandlers::try_new_coordinated(
                Arc::clone(&state_store),
            )
            .expect("coordinated handlers");
        let changes = DataCache::new(false);
        changes.add(
            StorageKey::new(5, vec![0xBB]),
            StorageItem::from_bytes(b"next-value".to_vec()),
        );
        assert!(handlers.on_committing(1, &changes));
        let canonical = fixture.canonical.clone();
        let authority_for_commit = Arc::clone(&authority);
        let roots = handlers
            .commit_pending_coordinated(|state_backing, prepared| {
                authority_for_commit.commit_prepared(prepared, |metadata, marker| {
                    let mut canonical_overlay =
                        TestOverlay(vec![(b"canonical-tip".to_vec(), Some(vec![1]))]);
                    canonical.commit_coordinated_overlays_with_required_marker(
                        &mut canonical_overlay,
                        state_backing,
                        metadata,
                        marker,
                    )
                })
            })
            .expect("authoritative coordinated commit")
            .expect("one committed root");
        assert_eq!(roots.len(), 1);
        let next_root = roots[0].to_array();
        let mut next_root_key = [0u8; PACK_KEY_BYTES];
        next_root_key[0] = 0xF0;
        next_root_key[1..].copy_from_slice(&next_root);
        assert!(
            authority
                .snapshot()
                .try_get_node_bytes(&next_root_key)
                .expect("read new pack root")
                .is_some()
        );
        assert_eq!(
            fixture.state.try_get_bytes(&next_root_key),
            None,
            "new exact node rows must not be duplicated into MDBX"
        );
        let marker = fixture
            .canonical
            .maintenance_metadata(AUTHORITATIVE_HIGH_WATER_KEY)
            .expect("read marker")
            .expect("marker committed");
        let marker = AuthoritativeHighWaterRecord::decode(&marker).expect("decode marker");
        assert_eq!((marker.block_index, marker.state_root), (1, next_root));

        let pack_bytes_before_revert = fs::metadata(fixture.pack_path.join("frames.pack"))
            .expect("stat pack before revert")
            .len();
        let authority_for_revert = Arc::clone(&authority);
        let canonical_for_revert = fixture.canonical.clone();
        handlers
            .on_reverting_coordinated(1, 1, |state_backing, prepared| {
                authority_for_revert.commit_prepared(prepared, |metadata, marker| {
                    let mut canonical_overlay =
                        TestOverlay(vec![(b"canonical-tip".to_vec(), Some(vec![0]))]);
                    canonical_for_revert.commit_coordinated_overlays_with_required_marker(
                        &mut canonical_overlay,
                        state_backing,
                        metadata,
                        marker,
                    )
                })
            })
            .expect("coordinated metadata-only revert");
        assert_eq!(
            state_store
                .mpt()
                .expect("MPT")
                .current_local_root()
                .map(|(index, root)| (index, root.to_array())),
            Some((0, fixture.root))
        );
        assert_eq!(
            fs::metadata(fixture.pack_path.join("frames.pack"))
                .expect("stat pack after revert")
                .len(),
            pack_bytes_before_revert,
            "full-state revert must only rebind metadata and marker"
        );
        let reverted_marker = fixture
            .canonical
            .maintenance_metadata(AUTHORITATIVE_HIGH_WATER_KEY)
            .expect("read reverted marker")
            .expect("reverted marker exists");
        let reverted_marker =
            AuthoritativeHighWaterRecord::decode(&reverted_marker).expect("decode reverted marker");
        assert_eq!(reverted_marker.epoch, marker.epoch);
        assert_eq!(
            (reverted_marker.block_index, reverted_marker.state_root),
            (0, fixture.root)
        );

        drop(handlers);
        drop(state_store);
        drop(authority_for_revert);
        drop(authority_for_commit);
        drop(authority);
        let reopened = AuthoritativeNodePack::open(
            &fixture.pack_path,
            64 * 1024 * 1024,
            0x334F_454E,
            &fixture.state,
        )
        .expect("reopen from mandatory marker");
        assert!(
            reopened
                .snapshot()
                .try_get_node_bytes(&next_root_key)
                .expect("read reopened root")
                .is_some()
        );
    }

    #[test]
    fn deferred_authority_materializes_two_windows_against_pinned_pack_generations() {
        let fixture = fixture();
        let classic_backing = copy_memory_store(&fixture.oracle);
        let oracle =
            neo_state_service::MptStore::from_memory_store(Arc::clone(&fixture.oracle), true)
                .expect("open eager oracle");
        let classic_deferred = neo_state_service::MptStore::from_memory_store_with_options(
            Arc::clone(&classic_backing),
            true,
            true,
        )
        .expect("open classic deferred oracle");
        let authority = AuthoritativeNodePack::open(
            &fixture.pack_path,
            64 * 1024 * 1024,
            0x334F_454E,
            &fixture.state,
        )
        .expect("open authority");
        let checkpoint_generation = authority.pinned_generation();
        assert_eq!(checkpoint_generation.sequence(), 0);
        let checkpoint_snapshot = checkpoint_generation.snapshot();
        let recording_factory = RecordingNodeFactory::new(Arc::clone(&authority));
        let factory: Arc<dyn MptNodeSnapshotFactory> = recording_factory.clone();
        let state_store = Arc::new(
            neo_state_service::StateStore::with_mpt_store_and_node_snapshot_options(
                true,
                true,
                Arc::new(fixture.state.clone()),
                factory,
            )
            .expect("open deferred split StateService"),
        );
        let handlers =
            neo_state_service::commit_handlers::StateServiceCommitHandlers::try_new_coordinated(
                Arc::clone(&state_store),
            )
            .expect("coordinated handlers");
        let mut observed_node_keys = exact_oracle_node_entries(&fixture.oracle)
            .into_iter()
            .map(|(key, _)| key)
            .collect::<BTreeSet<_>>();

        let first_values = [
            (StorageKey::new(5, vec![0xB0]), b"v0".to_vec()),
            (StorageKey::new(5, vec![0xB1]), b"v1".to_vec()),
            (StorageKey::new(7, vec![0x10]), b"x".to_vec()),
        ];
        let first_oracle_changes = first_values
            .iter()
            .map(|(key, value)| neo_state_service::MptChange::Put {
                key: key.to_array(),
                value: value.clone(),
            })
            .collect::<Vec<_>>();
        let first_oracle_root = oracle
            .apply_block_changes(
                1,
                Some(neo_primitives::UInt256::from(fixture.root)),
                &first_oracle_changes,
            )
            .expect("apply eager oracle window one");
        let first_classic_root = classic_deferred
            .apply_block_changes(
                1,
                Some(neo_primitives::UInt256::from(fixture.root)),
                &first_oracle_changes,
            )
            .expect("apply classic deferred oracle window one");
        assert_eq!(first_classic_root, first_oracle_root);
        assert_eq!(
            exact_oracle_node_entries(&classic_backing),
            exact_oracle_node_entries(&fixture.oracle)
        );
        let first_cache = DataCache::new(false);
        for (key, value) in &first_values {
            first_cache.add(key.clone(), StorageItem::from_bytes(value.clone()));
        }
        assert!(handlers.on_committing(1, &first_cache));
        let canonical = fixture.canonical.clone();
        let authority_for_first_commit = Arc::clone(&authority);
        let first_roots = handlers
            .commit_pending_coordinated(|state_backing, prepared| {
                let unresolved = prepared.unresolved_node_journal().len();
                assert!(unresolved > 0, "window one must export a deferred journal");
                authority_for_first_commit.commit_prepared(prepared, |metadata, marker| {
                    let mut canonical_overlay =
                        TestOverlay(vec![(b"canonical-tip".to_vec(), Some(vec![1]))]);
                    canonical.commit_coordinated_overlays_with_required_marker(
                        &mut canonical_overlay,
                        state_backing,
                        metadata,
                        marker,
                    )
                })?;
                assert!(prepared.unresolved_node_journal().is_empty());
                assert!(prepared.materialized_node_operation_count() >= unresolved);
                Ok(())
            })
            .expect("commit deferred authority window one")
            .expect("window one root");
        assert_eq!(first_roots, vec![first_oracle_root]);
        assert_eq!(authority.pinned_generation().sequence(), 1);
        let first_root = first_oracle_root.to_array();
        let mut first_root_key = [0u8; PACK_KEY_BYTES];
        first_root_key[0] = 0xF0;
        first_root_key[1..].copy_from_slice(&first_root);
        assert_eq!(
            checkpoint_snapshot
                .try_get_node_bytes(&first_root_key)
                .expect("read checkpoint generation"),
            None,
            "window-one root must not already exist in the checkpoint generation"
        );
        assert!(
            authority
                .snapshot()
                .try_get_node_bytes(&first_root_key)
                .expect("read window-one root")
                .is_some()
        );
        assert_pack_matches_oracle(&authority, &fixture.oracle, &mut observed_node_keys);
        assert_mdbx_node_namespace_empty(&fixture.state);

        let second_values = [
            (StorageKey::new(5, vec![0xB0]), b"v0".to_vec()),
            (StorageKey::new(6, vec![0x20]), b"y".to_vec()),
        ];
        let second_oracle_changes = second_values
            .iter()
            .map(|(key, value)| neo_state_service::MptChange::Put {
                key: key.to_array(),
                value: value.clone(),
            })
            .collect::<Vec<_>>();
        let second_oracle_root = oracle
            .apply_block_changes(2, Some(first_oracle_root), &second_oracle_changes)
            .expect("apply eager oracle window two");
        let second_classic_root = classic_deferred
            .apply_block_changes(2, Some(first_classic_root), &second_oracle_changes)
            .expect("apply classic deferred oracle window two");
        assert_eq!(second_classic_root, second_oracle_root);
        assert_eq!(
            exact_oracle_node_entries(&classic_backing),
            exact_oracle_node_entries(&fixture.oracle)
        );
        let second_cache = DataCache::new(false);
        for (key, value) in &second_values {
            second_cache.add(key.clone(), StorageItem::from_bytes(value.clone()));
        }
        assert!(handlers.on_committing(2, &second_cache));
        recording_factory.clear_reads();
        let canonical = fixture.canonical.clone();
        let authority_for_second_commit = Arc::clone(&authority);
        let second_roots = handlers
            .commit_pending_coordinated(|state_backing, prepared| {
                let unresolved = prepared.unresolved_node_journal().len();
                assert!(unresolved > 0, "window two must export a deferred journal");
                authority_for_second_commit.commit_prepared(prepared, |metadata, marker| {
                    let mut canonical_overlay =
                        TestOverlay(vec![(b"canonical-tip".to_vec(), Some(vec![2]))]);
                    canonical.commit_coordinated_overlays_with_required_marker(
                        &mut canonical_overlay,
                        state_backing,
                        metadata,
                        marker,
                    )
                })?;
                assert!(prepared.unresolved_node_journal().is_empty());
                assert!(prepared.materialized_node_operation_count() >= unresolved);
                Ok(())
            })
            .expect("commit deferred authority window two")
            .expect("window two root");
        assert_eq!(second_roots, vec![second_oracle_root]);
        assert_eq!(authority.pinned_generation().sequence(), 2);
        assert!(
            recording_factory.read_key_in_generation(1, &first_root_key),
            "window two must read window one's root from pack generation one"
        );
        assert_pack_matches_oracle(&authority, &fixture.oracle, &mut observed_node_keys);
        assert_mdbx_node_namespace_empty(&fixture.state);
        assert!(
            exact_oracle_node_entries(&fixture.oracle)
                .iter()
                .any(
                    |(_, value)| neo_crypto::mpt_trie::Node::split_serialized_reference(value)
                        .map(|(_, reference, _)| reference > 1)
                        .unwrap_or(false)
                ),
            "the repeated put must exercise persisted reference accumulation"
        );

        let second_root = second_oracle_root.to_array();
        assert_eq!(
            read_state_tip(&fixture.state).expect("read committed tip"),
            (2, second_root)
        );
        let marker = fixture
            .canonical
            .maintenance_metadata(AUTHORITATIVE_HIGH_WATER_KEY)
            .expect("read marker")
            .expect("marker committed");
        let marker = AuthoritativeHighWaterRecord::decode(&marker).expect("decode marker");
        assert_eq!((marker.block_index, marker.state_root), (2, second_root));

        drop(handlers);
        drop(state_store);
        drop(recording_factory);
        drop(authority_for_second_commit);
        drop(authority_for_first_commit);
        drop(authority);
        let reopened = AuthoritativeNodePack::open(
            &fixture.pack_path,
            64 * 1024 * 1024,
            0x334F_454E,
            &fixture.state,
        )
        .expect("reopen deferred authority from marker");
        assert_eq!(
            (
                reopened.publication.read().marker.block_index,
                reopened.publication.read().marker.state_root,
            ),
            (2, second_root)
        );
        assert_pack_matches_oracle(&reopened, &fixture.oracle, &mut observed_node_keys);
        assert_mdbx_node_namespace_empty(&fixture.state);
        let reopened_factory: Arc<dyn MptNodeSnapshotFactory> = reopened;
        let reopened_state =
            neo_state_service::StateStore::with_mpt_store_and_node_snapshot_options(
                true,
                true,
                Arc::new(fixture.state.clone()),
                reopened_factory,
            )
            .expect("reopen deferred split StateService");
        assert_eq!(
            reopened_state
                .mpt()
                .expect("reopened MPT")
                .current_local_root()
                .map(|(index, root)| (index, root.to_array())),
            Some((2, second_root))
        );
    }

    #[test]
    fn failed_mdbx_commit_keeps_old_generation_and_recovery_discards_sealed_suffix() {
        let fixture = fixture();
        let authority = AuthoritativeNodePack::open(
            &fixture.pack_path,
            64 * 1024 * 1024,
            0x334F_454E,
            &fixture.state,
        )
        .expect("open authority");
        let factory: Arc<dyn MptNodeSnapshotFactory> = authority.clone();
        let state_store = Arc::new(
            neo_state_service::StateStore::with_mpt_store_and_node_snapshots(
                true,
                Arc::new(fixture.state.clone()),
                factory,
            )
            .expect("open split StateService"),
        );
        let handlers =
            neo_state_service::commit_handlers::StateServiceCommitHandlers::try_new_coordinated(
                Arc::clone(&state_store),
            )
            .expect("coordinated handlers");
        let changes = DataCache::new(false);
        changes.add(
            StorageKey::new(5, vec![0xCC]),
            StorageItem::from_bytes(b"must-rollback".to_vec()),
        );
        assert!(handlers.on_committing(1, &changes));
        let authority_for_commit = Arc::clone(&authority);
        let error = handlers
            .commit_pending_coordinated(|_state_backing, prepared| {
                authority_for_commit.commit_prepared(prepared, |_metadata, _marker| {
                    Err(StorageError::commit_failed("injected MDBX failure"))
                })
            })
            .expect_err("failed MDBX commit must reject publication");
        assert!(error.contains("injected MDBX failure"), "{error}");
        assert_eq!(
            state_store
                .mpt()
                .expect("MPT")
                .current_local_root()
                .map(|(index, root)| (index, root.to_array())),
            Some((0, fixture.root))
        );
        assert_eq!(
            fixture
                .canonical
                .maintenance_metadata(AUTHORITATIVE_HIGH_WATER_KEY)
                .expect("read marker after failure"),
            None
        );

        drop(handlers);
        drop(state_store);
        drop(authority_for_commit);
        drop(authority);
        let recovered = AuthoritativeNodePack::open(
            &fixture.pack_path,
            64 * 1024 * 1024,
            0x334F_454E,
            &fixture.state,
        )
        .expect("checkpoint horizon discards sealed orphan suffix");
        let publication = recovered.publication.read();
        assert_eq!(publication.marker.epoch, 0);
        assert_eq!(publication.marker.block_index, 0);
    }

    #[test]
    fn metadata_only_block_advances_marker_without_pack_io() {
        let fixture = fixture();
        let pack_file = fixture.pack_path.join("frames.pack");
        let pack_bytes_before = fs::metadata(&pack_file).expect("stat pack before").len();
        let authority = AuthoritativeNodePack::open(
            &fixture.pack_path,
            64 * 1024 * 1024,
            0x334F_454E,
            &fixture.state,
        )
        .expect("open authority");
        let factory: Arc<dyn MptNodeSnapshotFactory> = authority.clone();
        let state_store = Arc::new(
            neo_state_service::StateStore::with_mpt_store_and_node_snapshots(
                true,
                Arc::new(fixture.state.clone()),
                factory,
            )
            .expect("open split StateService"),
        );
        let handlers =
            neo_state_service::commit_handlers::StateServiceCommitHandlers::try_new_coordinated(
                Arc::clone(&state_store),
            )
            .expect("coordinated handlers");
        assert!(handlers.on_committing(1, &DataCache::new(false)));
        let canonical = fixture.canonical.clone();
        let authority_for_commit = Arc::clone(&authority);
        let roots = handlers
            .commit_pending_coordinated(|state_backing, prepared| {
                authority_for_commit.commit_prepared(prepared, |metadata, marker| {
                    let mut canonical_overlay =
                        TestOverlay(vec![(b"canonical-tip".to_vec(), Some(vec![1]))]);
                    canonical.commit_coordinated_overlays_with_required_marker(
                        &mut canonical_overlay,
                        state_backing,
                        metadata,
                        marker,
                    )
                })
            })
            .expect("metadata-only commit")
            .expect("one root");
        assert_eq!(roots[0].to_array(), fixture.root);
        assert_eq!(
            fs::metadata(&pack_file).expect("stat pack after").len(),
            pack_bytes_before,
            "an empty block must append no pack frame"
        );
        let marker = fixture
            .canonical
            .maintenance_metadata(AUTHORITATIVE_HIGH_WATER_KEY)
            .expect("read marker")
            .expect("metadata-only marker committed");
        let marker = AuthoritativeHighWaterRecord::decode(&marker).expect("decode marker");
        assert_eq!(marker.epoch, 0);
        assert_eq!(marker.block_index, 1);
        assert_eq!(marker.state_root, fixture.root);
    }

    #[test]
    fn background_maintenance_builds_off_lock_and_publishes_equivalent_snapshot() {
        let temporary = tempdir().expect("temporary maintenance fixture");
        let pack_path = temporary.path().join("packs");
        let mut store = PackStore::create(&pack_path, 64 * 1024 * 1024).expect("create pack");
        let mut key = [0x55; PACK_KEY_BYTES];
        key[0] = 0xF0;
        let mut latest_snapshot = None;
        for epoch in 0..9u8 {
            let prepared = store
                .prepare_append(&[PackOperation {
                    key,
                    kind: PackOpKind::Put(vec![epoch]),
                }])
                .expect("prepare unmaintained frame");
            latest_snapshot = Some(
                store
                    .seal_prepared(prepared)
                    .expect("seal unmaintained frame")
                    .into_snapshot(),
            );
        }
        assert!(store.compaction_debt().excess_runs > 0);
        let receipt = store.last_frame_receipt().expect("pack receipt");
        let initial_generation = latest_snapshot
            .as_ref()
            .expect("latest snapshot")
            .generation();
        let writer = Arc::new(Mutex::new(Some(store)));
        let publication = Arc::new(RwLock::new(PublishedGeneration {
            sequence: 7,
            snapshot: Arc::new(PackNodeSnapshot {
                inner: latest_snapshot.expect("latest snapshot"),
            }),
            marker: AuthoritativeHighWaterRecord::new(
                0x334F_454E,
                [0x11; 32],
                receipt,
                9,
                [0x22; 32],
            ),
        }));
        let maintenance = PackMaintenance::spawn(
            Arc::clone(&writer),
            Arc::clone(&publication),
            pack_path.clone(),
        )
        .expect("spawn maintenance");
        loop {
            let debt = writer
                .lock()
                .as_ref()
                .expect("healthy writer")
                .compaction_debt();
            if debt.excess_runs == 0 {
                break;
            }
            let progress = maintenance.progress();
            maintenance.request().expect("request maintenance");
            maintenance
                .wait_for_progress(progress)
                .expect("wait for maintenance");
        }
        let published = publication.read();
        assert_eq!(published.sequence, 7, "derived work is not a state epoch");
        assert!(published.snapshot.inner.generation() > initial_generation);
        assert_eq!(
            published
                .snapshot
                .try_get_node_bytes(&key)
                .expect("read compacted snapshot"),
            Some(vec![8])
        );
        drop(published);
        assert!(
            writer
                .lock()
                .as_ref()
                .expect("healthy writer")
                .compaction_stats()
                .cycles
                > 0
        );
        drop(maintenance);
        drop(publication);
        drop(writer);
        let reopened = PackStore::open(&pack_path, 64 * 1024 * 1024).expect("reopen pack");
        assert_eq!(
            reopened.get(&key).expect("read reopened pack"),
            Some(vec![8])
        );
    }
}
