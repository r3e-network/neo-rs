//! Cold-first authoritative pack manager for exact StateService MPT nodes.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::mpsc::{Receiver, SyncSender, TrySendError, sync_channel};
use std::thread::{self, JoinHandle};

use anyhow::{Context, ensure};
use neo_state_packs::authority::{AUTHORITATIVE_HIGH_WATER_KEY, AuthoritativeHighWaterRecord};
use neo_state_packs::checkpoint::PackCheckpoint;
use neo_state_packs::{
    PACK_KEY_BYTES, PackFrameContext, PackStore, PackStoreConfig, PackStoreError, PackStoreOptions,
    Snapshot as PackSnapshot,
};
use neo_state_service::mpt_store::{PreparedMptCommit, PreparedMptMetadataOverlay};
use neo_state_service::{
    MptNodeReadGeneration, MptNodeReadSnapshot, MptNodeSnapshotFactory,
    read_current_local_root_from, read_local_state_root,
};
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::{
    CoordinatedCommitMarker, RawReadOnlyStore, Store, TransactionalStore,
};
use neo_storage::{StorageError, StorageResult};
use parking_lot::{Condvar, Mutex, RwLock};
use tracing::{error, warn};

fn validate_authoritative_transition(
    base_marker: AuthoritativeHighWaterRecord,
    block_index: u32,
    state_root: [u8; 32],
    materialized_node_operations: Option<usize>,
) -> StorageResult<()> {
    if block_index < base_marker.block_index {
        return Err(StorageError::invalid_operation(format!(
            "authoritative node packs cannot publish canonical rewind from block {} to block {} until a branch-isolated pack horizon is available",
            base_marker.block_index, block_index
        )));
    }
    if block_index == base_marker.block_index && state_root != base_marker.state_root {
        return Err(StorageError::invalid_operation(format!(
            "authoritative node packs cannot replace the state root at canonical block {block_index}"
        )));
    }
    if materialized_node_operations == Some(0) && state_root != base_marker.state_root {
        return Err(StorageError::invalid_operation(format!(
            "authoritative node packs cannot advance to block {block_index} with a changed state root and an empty node overlay"
        )));
    }
    if materialized_node_operations.is_some_and(|operations| operations > 0)
        && block_index <= base_marker.block_index
    {
        return Err(StorageError::invalid_operation(format!(
            "authoritative node packs cannot bind a non-empty frame to block {block_index} after canonical block {}",
            base_marker.block_index
        )));
    }
    Ok(())
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
    deferred: Option<String>,
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
        if self.state.0.lock().deferred.is_some() {
            return Ok(());
        }
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
        while state.progress == observed && state.failure.is_none() && state.deferred.is_none() {
            wake.wait(&mut state);
        }
        if let Some(error) = &state.failure {
            return Err(StorageError::backend(format!(
                "authoritative pack maintenance failed: {error}"
            )));
        }
        if let Some(reason) = &state.deferred {
            return Err(StorageError::backend(format!(
                "authoritative pack maintenance is deferred: {reason}"
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
    'requests: while receiver.recv().is_ok() {
        if state.0.lock().deferred.is_some() {
            continue;
        }
        let mut adopted = false;
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
            let prepared = match plan.build() {
                Ok(prepared) => prepared,
                Err(error) => {
                    let Some(PackStoreError::CompactionWorkspaceExceeded {
                        estimated_bytes,
                        max_bytes,
                    }) = error.downcast_ref::<PackStoreError>()
                    else {
                        return Err(error);
                    };
                    let reason = format!(
                        "estimated workspace {estimated_bytes} bytes exceeds configured bound {max_bytes} bytes"
                    );
                    warn!(
                        target: "neo::state_packs",
                        estimated_bytes,
                        max_bytes,
                        "authoritative derived-index compaction deferred before allocation"
                    );
                    defer_maintenance(state, reason);
                    continue 'requests;
                }
            };
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
            adopted = true;
            note_maintenance_progress(state);
            thread::yield_now();
        }
        if adopted {
            let mut writer = writer.lock();
            writer
                .as_mut()
                .context("authoritative pack writer is unavailable")?
                .gc()?;
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

fn defer_maintenance(state: &(Mutex<MaintenanceState>, Condvar), reason: String) {
    let (state, wake) = state;
    let mut state = state.lock();
    state.deferred = Some(reason);
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
        const MAX_NODE_VALUE_BYTES: u64 = 1024 * 1024;
        const MAX_BATCH_VALUE_BYTES: u64 = 256 * 1024 * 1024;
        self.inner
            .get_many_sorted_bounded(&keys, MAX_NODE_VALUE_BYTES, MAX_BATCH_VALUE_BYTES)
            .map_err(|error| {
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
    #[cfg(test)]
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
    #[cfg(test)]
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
                ..PackStoreOptions::default()
            },
        )
    }

    pub(in crate::node) fn open_with_options(
        path: &Path,
        max_index_memory_bytes: u64,
        network_magic: u32,
        state_backing: &RuntimeStore,
        options: PackStoreOptions,
    ) -> anyhow::Result<Arc<Self>> {
        let pack_config = PackStoreConfig::default()
            .with_max_index_memory_bytes(max_index_memory_bytes)
            .context("validate authoritative pack index-memory bound")?
            .with_read_options(options)
            .context("validate authoritative pack read options")?;
        let checkpoint = PackCheckpoint::read(path).context("read authoritative checkpoint")?;
        let checkpoint_binding = checkpoint.validate_authoritative(network_magic)?;
        let store_identity = checkpoint_binding.store_identity();
        let checkpoint_root = checkpoint_binding.source_root_internal();
        let checkpoint_frame_digest = checkpoint_binding.tip_frame_sha256();
        let state_snapshot = state_backing.snapshot();
        let state_root = read_current_local_root_from(state_snapshot.as_ref())
            .context("read current local StateService root from startup snapshot")?;
        let state_tip = (state_root.index(), state_root.root_hash().to_array());
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
                validate_authoritative_frame_history(
                    state_snapshot.as_ref(),
                    marker,
                    checkpoint.source_height,
                    checkpoint_root,
                )?;
                let store = PackStore::open_at_commit_horizon(
                    path,
                    pack_config,
                    Some(marker.commit_horizon()),
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
                let checkpoint_horizon = checkpoint_binding.commit_horizon();
                let store =
                    PackStore::open_at_commit_horizon(path, pack_config, Some(checkpoint_horizon))
                        .with_context(|| {
                            format!("open authoritative checkpoint {}", path.display())
                        })?;
                let receipt = store
                    .last_frame_receipt()
                    .context("authoritative checkpoint has no pack tip")?;
                ensure!(
                    receipt.epoch == checkpoint.tip_epoch
                        && receipt.segment_id.get() == checkpoint.tip_segment_id
                        && receipt.frame_end == checkpoint.tip_frame_end
                        && receipt.context == checkpoint_horizon.context
                        && receipt.frame_sha256 == checkpoint_frame_digest,
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
        let base_marker = self.publication.read().marker;
        let block_index = prepared.block_index();
        let state_root = prepared.root_hash().to_array();
        validate_authoritative_transition(base_marker, block_index, state_root, None)?;
        prepared.materialize_deferred_node_overlay()?;
        let expected_operations = prepared.materialized_node_operation_count();
        let expected_value_bytes = prepared.materialized_node_value_bytes();
        validate_authoritative_transition(
            base_marker,
            block_index,
            state_root,
            Some(expected_operations),
        )?;
        let has_operations = expected_operations > 0;

        let mut writer = loop {
            self.maintenance.ensure_healthy()?;
            let observed_progress = self.maintenance.progress();
            let writer = self.writer.lock();
            let store = writer.as_ref().ok_or_else(|| {
                StorageError::backend(
                    "authoritative pack writer is poisoned; restart for marker recovery",
                )
            })?;
            if !has_operations || !store.compaction_debt().backpressure_required {
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
        let sealed = if !has_operations {
            // The coordinated commit contract requires both overlay halves to
            // be consumed, including an empty node overlay.
            prepared.visit_materialized_node_overlay(&mut |_key: &[u8], _value: Option<&[u8]>| {});
            None
        } else {
            let block_start = base_marker.block_index.checked_add(1).ok_or_else(|| {
                StorageError::invalid_operation(
                    "authoritative frame block range overflows after u32::MAX",
                )
            })?;
            let context =
                PackFrameContext::new(block_start, block_index, base_marker.state_root, state_root);
            let mut frame_builder = store
                .frame_builder_with_value_bytes(context, expected_operations, expected_value_bytes)
                .map_err(|error| {
                    StorageError::invalid_operation(format!(
                        "authoritative pack frame initialization failed: {error:#}"
                    ))
                })?;
            let mut conversion_error = None;
            prepared.visit_materialized_node_overlay(&mut |key: &[u8], value: Option<&[u8]>| {
                if conversion_error.is_some() {
                    return;
                }
                match exact_node_key(key) {
                    Ok(key) => {
                        if let Err(error) = frame_builder.push_key(key, value) {
                            conversion_error = Some(StorageError::invalid_operation(format!(
                                "authoritative pack frame encoding failed: {error:#}"
                            )));
                        }
                    }
                    Err(error) => conversion_error = Some(error),
                }
            });
            if let Some(error) = conversion_error {
                return Err(error);
            }
            if frame_builder.len() != expected_operations {
                return Err(StorageError::invalid_operation(
                    "authoritative node overlay conversion omitted an operation",
                ));
            }
            let pending = match store.prepare_built_append(frame_builder) {
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
        let maintenance_needed = has_operations && store.compaction_debt().excess_runs > 0;
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

#[cfg(test)]
fn read_state_tip(store: &RuntimeStore) -> anyhow::Result<(u32, [u8; 32])> {
    let root = neo_state_service::read_current_local_root(store)
        .context("read current local StateService root")?;
    Ok((root.index(), root.root_hash().to_array()))
}

fn validate_authoritative_frame_history<R>(
    snapshot: &R,
    marker: AuthoritativeHighWaterRecord,
    checkpoint_height: u32,
    checkpoint_root: [u8; 32],
) -> anyhow::Result<()>
where
    R: RawReadOnlyStore + ?Sized,
{
    let context = marker.frame_context;
    ensure!(
        context.block_end <= marker.block_index,
        "authoritative frame ends after the canonical StateService tip"
    );
    let resulting = read_local_state_root(snapshot, context.block_end).with_context(|| {
        format!(
            "read authoritative frame resulting root at block {}",
            context.block_end
        )
    })?;
    ensure!(
        resulting.root_hash().to_array() == context.resulting_root,
        "authoritative frame resulting root differs from StateService history at block {}",
        context.block_end
    );

    let checkpoint_context = PackFrameContext::new(
        checkpoint_height,
        checkpoint_height,
        checkpoint_root,
        checkpoint_root,
    );
    if context == checkpoint_context {
        return Ok(());
    }

    if context.block_start == 0 {
        ensure!(
            context.previous_root == [0; 32],
            "authoritative genesis frame must bind the zero previous root"
        );
        return Ok(());
    }

    let previous_index = context.block_start - 1;
    let previous = read_local_state_root(snapshot, previous_index).with_context(|| {
        format!("read authoritative frame previous root at block {previous_index}")
    })?;
    ensure!(
        previous.root_hash().to_array() == context.previous_root,
        "authoritative frame previous root differs from StateService history at block {previous_index}"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_state_packs::checkpoint::{
        PACK_CHECKPOINT_SCHEMA_VERSION, PACK_CHECKPOINT_SOURCE_NAMESPACE, PackCheckpoint,
    };
    use neo_state_packs::{
        PACK_FRAME_FORMAT_VERSION, PACK_INDEX_FORMAT_VERSION, PACK_MANIFEST_FORMAT_VERSION,
        PACK_SEGMENT_FORMAT_VERSION, PACK_SEGMENT_HEADER_LEN, PackOpKind, PackOperation,
        PackSegmentId,
    };
    use neo_storage::mdbx::MdbxStoreProvider;
    use neo_storage::persistence::providers::MemoryStore;
    use neo_storage::persistence::storage::StorageConfig;
    use neo_storage::persistence::{
        RawOverlaySink, RawOverlaySource, ReadOnlyStoreGeneric, SeekDirection, WriteStore,
    };
    use neo_storage::{DataCache, StorageItem, StorageKey};
    use serde_json::json;
    use std::collections::BTreeSet;
    use std::fs;
    use tempfile::tempdir;

    fn test_pack_config(max_index_memory_bytes: u64) -> PackStoreConfig {
        PackStoreConfig::default()
            .with_max_index_memory_bytes(max_index_memory_bytes)
            .expect("valid authoritative pack test configuration")
    }

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
        let mut pack =
            PackStore::create(&pack_path, test_pack_config(64 * 1024 * 1024)).expect("create pack");
        pack.append_frame(PackFrameContext::new(0, 0, root, root), &node_operations)
            .expect("append source nodes");
        let scrub = pack
            .scrub_checkpoint_namespace()
            .expect("scrub fixture checkpoint");
        let (pack_bytes, live_index_bytes, live_runs, decoded_index_memory_bytes) =
            pack.layout().expect("fixture pack layout");
        let receipt = pack.last_frame_receipt().expect("pack receipt");
        drop(pack);
        let value_bytes = node_operations
            .iter()
            .map(|operation| match &operation.kind {
                PackOpKind::Put(value) => value.len() as u64,
                PackOpKind::Tombstone => 0,
            })
            .sum();
        let mut display_root = root;
        display_root.reverse();
        let checkpoint = PackCheckpoint {
            schema_version: PACK_CHECKPOINT_SCHEMA_VERSION,
            authoritative_ready: true,
            complete: true,
            source_backend: "mdbx".to_owned(),
            source_namespace: PACK_CHECKPOINT_SOURCE_NAMESPACE.to_owned(),
            network_magic: "0x334F454E".to_owned(),
            source_height: 0,
            source_root: format!("0x{}", hex::encode(display_root)),
            source_root_internal_bytes: format!("0x{}", hex::encode(root)),
            source_namespace_sha256: format!("0x{}", hex::encode(scrub.sha256)),
            rows: node_operations.len() as u64,
            resumed_rows: 0,
            value_bytes,
            frames: 1,
            rows_per_frame: node_operations.len(),
            pack_bytes,
            live_index_bytes,
            live_runs,
            decoded_index_memory_bytes,
            gc_runs_deleted: 0,
            gc_manifests_deleted: 0,
            gc_bytes_reclaimed: 0,
            pack_segment_format_version: PACK_SEGMENT_FORMAT_VERSION,
            pack_frame_format_version: PACK_FRAME_FORMAT_VERSION,
            pack_index_format_version: PACK_INDEX_FORMAT_VERSION,
            pack_manifest_format_version: PACK_MANIFEST_FORMAT_VERSION,
            tip_epoch: receipt.epoch,
            tip_segment_id: receipt.segment_id.get(),
            tip_frame_end: receipt.frame_end,
            tip_frame_sha256: format!("0x{}", hex::encode(receipt.frame_sha256)),
            scrubbed_frames: scrub.scrub.frames,
            scrubbed_rows: scrub.scrub.rows,
            scrubbed_puts: scrub.scrub.puts,
            scrubbed_tombstones: scrub.scrub.tombstones,
            scrubbed_payload_bytes: scrub.scrub.payload_bytes,
            scrubbed_value_bytes: scrub.scrub.value_bytes,
            scrub_elapsed_seconds: 0.0,
            elapsed_seconds: 0.0,
        };
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

    #[test]
    fn authoritative_checkpoint_rejects_old_schema_and_missing_segment_identity() {
        let fixture = fixture();
        let checkpoint_path = fixture.pack_path.join("checkpoint.json");
        let original: serde_json::Value =
            serde_json::from_slice(&fs::read(&checkpoint_path).expect("read authority checkpoint"))
                .expect("decode authority checkpoint");

        let mut old_schema = original.clone();
        old_schema["schema_version"] = json!(PACK_CHECKPOINT_SCHEMA_VERSION - 1);
        fs::write(
            &checkpoint_path,
            serde_json::to_vec_pretty(&old_schema).expect("encode old checkpoint schema"),
        )
        .expect("write old checkpoint schema");
        let checkpoint =
            PackCheckpoint::read(&fixture.pack_path).expect("decode complete old schema");
        assert!(
            checkpoint
                .validate_authoritative(0x334F_454E)
                .expect_err("old checkpoint schema must fail")
                .to_string()
                .contains("unsupported")
        );

        for missing in ["pack_segment_format_version", "tip_segment_id"] {
            let mut incomplete = original.clone();
            incomplete
                .as_object_mut()
                .expect("checkpoint object")
                .remove(missing);
            fs::write(
                &checkpoint_path,
                serde_json::to_vec_pretty(&incomplete).expect("encode incomplete checkpoint"),
            )
            .expect("write incomplete checkpoint");
            let error = PackCheckpoint::read(&fixture.pack_path)
                .expect_err("missing segment identity must fail decoding");
            assert!(format!("{error:#}").contains(missing));
        }

        let mut header_position = original.clone();
        header_position["tip_frame_end"] = json!(PACK_SEGMENT_HEADER_LEN);
        fs::write(
            &checkpoint_path,
            serde_json::to_vec_pretty(&header_position).expect("encode header checkpoint tip"),
        )
        .expect("write header checkpoint tip");
        let checkpoint = PackCheckpoint::read(&fixture.pack_path)
            .expect("decode complete header checkpoint tip");
        assert!(
            checkpoint
                .validate_authoritative(0x334F_454E)
                .expect_err("checkpoint tip inside its segment header must fail")
                .to_string()
                .contains("segment header")
        );

        let mut impossible_segment = original;
        let tip_epoch = impossible_segment["tip_epoch"]
            .as_u64()
            .expect("checkpoint tip epoch");
        impossible_segment["tip_segment_id"] = json!(tip_epoch + 1);
        fs::write(
            &checkpoint_path,
            serde_json::to_vec_pretty(&impossible_segment)
                .expect("encode impossible checkpoint segment"),
        )
        .expect("write impossible checkpoint segment");
        let checkpoint = PackCheckpoint::read(&fixture.pack_path)
            .expect("decode complete impossible checkpoint segment");
        assert!(
            checkpoint
                .validate_authoritative(0x334F_454E)
                .expect_err("segment after the tip epoch must fail")
                .to_string()
                .contains("after the tip epoch")
        );
    }

    #[test]
    fn authoritative_transition_rejects_unbound_root_changes() {
        let base = AuthoritativeHighWaterRecord {
            network_magic: 0x334F_454E,
            store_identity: [0x11; 32],
            epoch: 7,
            segment_id: PackSegmentId::INITIAL,
            frame_end: 4_096,
            frame_sha256: [0x22; 32],
            frame_context: PackFrameContext::new(40, 42, [0x10; 32], [0x33; 32]),
            block_index: 42,
            state_root: [0x33; 32],
        };

        let rewind = validate_authoritative_transition(base, 41, base.state_root, None)
            .expect_err("backward height must fail closed");
        assert!(rewind.to_string().contains("canonical rewind"));
        let replacement = validate_authoritative_transition(base, 42, [0x44; 32], None)
            .expect_err("same-height root replacement must fail closed");
        assert!(replacement.to_string().contains("replace the state root"));
        let empty_changed = validate_authoritative_transition(base, 43, [0x44; 32], Some(0))
            .expect_err("changed root without node mutations must fail closed");
        assert!(empty_changed.to_string().contains("empty node overlay"));

        validate_authoritative_transition(base, 43, base.state_root, Some(0))
            .expect("metadata-only forward block keeps the root");
        validate_authoritative_transition(base, 43, [0x44; 32], Some(1))
            .expect("a changed root may be bound to materialized node mutations");
    }

    #[test]
    fn authoritative_frame_history_binds_result_and_previous_roots() {
        let mut roots = MemoryStore::new();
        let previous_root = [0x44; 32];
        let resulting_root = [0x55; 32];
        for (index, root) in [(4, previous_root), (5, resulting_root)] {
            roots
                .put(
                    neo_state_service::Keys::state_root(index),
                    neo_state_service::StateRoot::new_current(
                        index,
                        neo_primitives::UInt256::from(root),
                    )
                    .to_array(),
                )
                .expect("write historical StateService root");
        }
        let marker = AuthoritativeHighWaterRecord {
            network_magic: 0x334F_454E,
            store_identity: [0x11; 32],
            epoch: 7,
            segment_id: PackSegmentId::INITIAL,
            frame_end: 4_096,
            frame_sha256: [0x22; 32],
            frame_context: PackFrameContext::new(5, 5, previous_root, resulting_root),
            block_index: 6,
            state_root: resulting_root,
        };
        validate_authoritative_frame_history(&roots, marker, 0, [0x10; 32])
            .expect("valid incremental frame history");

        roots
            .put(
                neo_state_service::Keys::state_root(5),
                neo_state_service::StateRoot::new_current(
                    5,
                    neo_primitives::UInt256::from([0x66; 32]),
                )
                .to_array(),
            )
            .expect("replace resulting root");
        let error = validate_authoritative_frame_history(&roots, marker, 0, [0x10; 32])
            .expect_err("historical resulting-root mismatch must fail");
        assert!(error.to_string().contains("resulting root differs"));

        roots
            .put(
                neo_state_service::Keys::state_root(5),
                neo_state_service::StateRoot::new_current(
                    5,
                    neo_primitives::UInt256::from(resulting_root),
                )
                .to_array(),
            )
            .expect("restore resulting root");
        roots
            .put(
                neo_state_service::Keys::state_root(4),
                neo_state_service::StateRoot::new_current(
                    4,
                    neo_primitives::UInt256::from([0x77; 32]),
                )
                .to_array(),
            )
            .expect("replace previous root");
        let error = validate_authoritative_frame_history(&roots, marker, 0, [0x10; 32])
            .expect_err("historical previous-root mismatch must fail");
        assert!(error.to_string().contains("previous root differs"));
    }

    #[test]
    fn authoritative_checkpoint_context_is_an_explicit_snapshot_anchor() {
        let mut roots = MemoryStore::new();
        let checkpoint_root = [0x55; 32];
        roots
            .put(
                neo_state_service::Keys::state_root(5),
                neo_state_service::StateRoot::new_current(
                    5,
                    neo_primitives::UInt256::from(checkpoint_root),
                )
                .to_array(),
            )
            .expect("write checkpoint root");
        let marker = AuthoritativeHighWaterRecord {
            network_magic: 0x334F_454E,
            store_identity: [0x11; 32],
            epoch: 7,
            segment_id: PackSegmentId::INITIAL,
            frame_end: 4_096,
            frame_sha256: [0x22; 32],
            frame_context: PackFrameContext::new(5, 5, checkpoint_root, checkpoint_root),
            block_index: 8,
            state_root: checkpoint_root,
        };
        validate_authoritative_frame_history(&roots, marker, 5, checkpoint_root)
            .expect("checkpoint frame binds a snapshot rather than a block transition");
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
    fn authoritative_rewind_fails_closed_without_moving_marker_or_snapshot() {
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

        let pack_bytes_before_revert =
            fs::metadata(fixture.pack_path.join(PackSegmentId::INITIAL.file_name()))
                .expect("stat pack before revert")
                .len();
        let error = handlers
            .on_reverting_coordinated(1, 1, |state_backing, prepared| {
                let _ = state_backing;
                authority.commit_prepared(prepared, |_metadata, _marker| {
                    panic!("canonical rewind must fail before the MDBX callback")
                })
            })
            .expect_err("authoritative canonical rewind must fail closed");
        assert!(
            error.contains("cannot publish canonical rewind from block 1 to block 0"),
            "{error}"
        );
        assert_eq!(
            state_store
                .mpt()
                .expect("MPT")
                .current_local_root()
                .map(|(index, root)| (index, root.to_array())),
            Some((1, next_root)),
            "failed rewind must leave the visible StateService tip unchanged"
        );
        assert_eq!(
            fs::metadata(fixture.pack_path.join(PackSegmentId::INITIAL.file_name()),)
                .expect("stat pack after revert")
                .len(),
            pack_bytes_before_revert,
            "rejected rewind must not append pack data"
        );
        let retained_marker = fixture
            .canonical
            .maintenance_metadata(AUTHORITATIVE_HIGH_WATER_KEY)
            .expect("read retained marker")
            .expect("retained marker exists");
        let retained_marker =
            AuthoritativeHighWaterRecord::decode(&retained_marker).expect("decode retained marker");
        assert_eq!(retained_marker, marker);
        assert!(
            authority
                .snapshot()
                .try_get_node_bytes(&next_root_key)
                .expect("read retained root")
                .is_some(),
            "failed rewind must leave the published node generation unchanged"
        );

        drop(handlers);
        drop(state_store);
        drop(authority_for_commit);
        drop(authority);
        let reopened = AuthoritativeNodePack::open(
            &fixture.pack_path,
            64 * 1024 * 1024,
            0x334F_454E,
            &fixture.state,
        )
        .expect("reopen from retained mandatory marker");
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
        let pack_file = fixture.pack_path.join(PackSegmentId::INITIAL.file_name());
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
        let mut store =
            PackStore::create(&pack_path, test_pack_config(64 * 1024 * 1024)).expect("create pack");
        let mut key = [0x55; PACK_KEY_BYTES];
        key[0] = 0xF0;
        let mut latest_snapshot = None;
        for epoch in 0..9u8 {
            let previous_root = epoch
                .checked_sub(1)
                .map_or([0; 32], |previous| [previous; 32]);
            let prepared = store
                .prepare_frame(
                    PackFrameContext::new(
                        u32::from(epoch),
                        u32::from(epoch),
                        previous_root,
                        [epoch; 32],
                    ),
                    &[PackOperation {
                        key,
                        kind: PackOpKind::Put(vec![epoch]),
                    }],
                )
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
        let (live_runs, gc_cycles) = {
            let writer = writer.lock();
            let store = writer.as_ref().expect("healthy writer");
            (
                store.layout().expect("read compacted layout").2,
                store.compaction_stats().gc_cycles,
            )
        };
        let manifest_files = fs::read_dir(&pack_path)
            .expect("read pack directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("manifest-") && name.ends_with(".man"))
            })
            .count();
        let run_files = fs::read_dir(pack_path.join("runs"))
            .expect("read run directory")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .path()
                    .extension()
                    .is_some_and(|extension| extension == "idx")
            })
            .count();
        assert_eq!(gc_cycles, 1, "the complete worker batch runs GC once");
        assert_eq!(manifest_files, 1, "worker leaves one live manifest");
        assert_eq!(run_files as u64, live_runs, "worker leaves only live runs");
        drop(maintenance);
        drop(publication);
        drop(writer);
        let reopened =
            PackStore::open(&pack_path, test_pack_config(64 * 1024 * 1024)).expect("reopen pack");
        assert_eq!(
            reopened.get(&key).expect("read reopened pack"),
            Some(vec![8])
        );
    }

    #[test]
    fn over_budget_maintenance_defers_without_poisoning_or_rescheduling() {
        let temporary = tempdir().expect("temporary maintenance fixture");
        let pack_path = temporary.path().join("packs");
        let mut store = PackStore::create(&pack_path, test_pack_config(64 * 1024))
            .expect("create bounded pack");
        for epoch in 0..9u64 {
            let operations: Vec<_> = (0..512u64)
                .map(|ordinal| {
                    let mut key = [0u8; PACK_KEY_BYTES];
                    key[0] = 0xF0;
                    key[1..9].copy_from_slice(&epoch.to_be_bytes());
                    key[9..17].copy_from_slice(&ordinal.to_be_bytes());
                    PackOperation {
                        key,
                        kind: PackOpKind::Put(vec![epoch as u8]),
                    }
                })
                .collect();
            let prepared = store
                .prepare_frame(
                    PackFrameContext::new(
                        u32::try_from(epoch).expect("test epoch fits u32"),
                        u32::try_from(epoch).expect("test epoch fits u32"),
                        epoch
                            .checked_sub(1)
                            .map_or([0; 32], |previous| [previous as u8; 32]),
                        [epoch as u8; 32],
                    ),
                    &operations,
                )
                .expect("prepare unmaintained frame");
            drop(
                store
                    .seal_prepared(prepared)
                    .expect("seal unmaintained frame")
                    .into_snapshot(),
            );
        }
        assert!(store.compaction_debt().excess_runs > 0);
        let receipt = store.last_frame_receipt().expect("pack receipt");
        let snapshot = Arc::new(PackNodeSnapshot {
            inner: Arc::new(store.snapshot().expect("pin source generation")),
        });
        let writer = Arc::new(Mutex::new(Some(store)));
        let publication = Arc::new(RwLock::new(PublishedGeneration {
            sequence: 0,
            snapshot,
            marker: AuthoritativeHighWaterRecord::new(
                0x334F_454E,
                [0x11; 32],
                receipt,
                9,
                [0x22; 32],
            ),
        }));
        let maintenance =
            PackMaintenance::spawn(Arc::clone(&writer), Arc::clone(&publication), pack_path)
                .expect("spawn maintenance");

        let observed = maintenance.progress();
        let error = maintenance
            .wait_for_progress(observed)
            .expect_err("over-budget maintenance must be reported as deferred");
        assert!(error.to_string().contains("deferred"));
        assert!(
            writer.lock().is_some(),
            "deferral must not poison the writer"
        );
        let deferred_progress = maintenance.progress();
        maintenance
            .request()
            .expect("a deferred request is coalesced without rescheduling");
        assert_eq!(
            maintenance.progress(),
            deferred_progress,
            "the same over-budget plan must not enter a retry loop"
        );
    }
}
