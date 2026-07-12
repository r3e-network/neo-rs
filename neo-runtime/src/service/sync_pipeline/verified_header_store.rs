//! Durable verified-header staging stores for header sync.
//!
//! An active header stage window anchors at one canonical height and advances a
//! fixed ahead-of-tip prefix toward a fixed target. Store-backed variants keep
//! staged headers, target metadata, and the `Headers` checkpoint in isolated
//! maintenance metadata so the checkpoint never outruns the records it claims.

use std::collections::BTreeMap;
use std::sync::Arc;

use neo_payloads::Header;
use neo_primitives::UInt256;
use neo_storage::persistence::providers::memory_store::MemoryStore;
use neo_storage::persistence::{StoreMaintenanceBatch, TableProvider, TransactionalStore};
use parking_lot::RwLock;

use super::checkpoint_store::{
    commit_maintenance, read_checkpoint, table_read_error, write_checkpoint,
};
use super::tables::{
    StoredVerifiedHeader, SyncCheckpointTable, VerifiedHeaderTable, VerifiedHeaderTargetHashTable,
    VerifiedHeaderWindowTable,
};
use super::{SyncStageCheckpoint, SyncStageCheckpointStore, SyncStageKind};
use crate::{ServiceError, ServiceResult};

/// Maximum verified headers retained ahead of the canonical tip.
///
/// This matches Neo's bounded live `HeaderCache` policy without making
/// `neo-runtime` depend on the concrete blockchain crate.
pub const MAX_VERIFIED_HEADER_WINDOW: u32 = 10_000;

/// Canonical anchor and fixed target for one active header staging window.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct HeaderStageWindow {
    /// Canonical header height at which the active window was created.
    pub base_height: u32,
    /// Fixed ahead-of-tip target for this window.
    pub target_height: u32,
    /// Hash of the target header once the staged prefix durably reaches it.
    pub target_hash: Option<UInt256>,
}

/// Provider-neutral verified-header staging seam for the `Headers` sync stage.
///
/// Implementations keep the `Headers` checkpoint truthful while a fixed
/// verified-header window is active. The trait extends the general checkpoint
/// seam so callers can share one store handle across stage progress and header
/// sidecar operations.
pub trait VerifiedHeaderStore: SyncStageCheckpointStore {
    /// Reads the active verified-header window, if any.
    fn window(&self) -> ServiceResult<Option<HeaderStageWindow>>;

    /// Reads one staged verified header by height.
    fn header(&self, height: u32) -> ServiceResult<Option<Header>>;

    /// Starts a fixed verified-header window at `base_height + 1..=target_height`.
    ///
    /// Repeated calls with the same active window are idempotent. A different
    /// active window is rejected until callers explicitly reset it.
    fn begin_window(
        &self,
        base_height: u32,
        target_height: u32,
    ) -> ServiceResult<HeaderStageWindow>;

    /// Resets the active verified-header window and clears any staged headers.
    fn reset_window(
        &self,
        base_height: u32,
        target_height: u32,
    ) -> ServiceResult<HeaderStageWindow>;

    /// Atomically commits a nonempty contiguous verified-header prefix.
    fn commit_verified_headers(&self, headers: &[Header]) -> ServiceResult<SyncStageCheckpoint>;

    /// Finishes a completed window once canonical import reaches `canonical_height`.
    fn finish_window(&self, canonical_height: u32) -> ServiceResult<SyncStageCheckpoint>;

    /// Discards an active sidecar window in favor of an authoritative canonical tip.
    ///
    /// Recovery uses this when another valid import path advanced the canonical
    /// ledger past an incomplete or divergent sidecar window.
    fn discard_window(&self, canonical_height: u32) -> ServiceResult<SyncStageCheckpoint>;
}

#[derive(Debug, Default)]
struct InMemoryVerifiedHeaderState {
    checkpoints: BTreeMap<SyncStageKind, SyncStageCheckpoint>,
    headers: BTreeMap<u32, Header>,
    window: Option<HeaderStageWindow>,
}

/// In-memory verified-header store for tests and non-persistent composition.
#[derive(Debug, Default)]
pub struct InMemoryVerifiedHeaderStore {
    state: RwLock<InMemoryVerifiedHeaderState>,
}

impl SyncStageCheckpointStore for InMemoryVerifiedHeaderStore {
    fn checkpoint(&self, stage: SyncStageKind) -> ServiceResult<Option<SyncStageCheckpoint>> {
        Ok(self.state.read().checkpoints.get(&stage).cloned())
    }

    fn put_checkpoint(&self, checkpoint: SyncStageCheckpoint) -> ServiceResult<()> {
        self.state
            .write()
            .checkpoints
            .insert(checkpoint.stage, checkpoint);
        Ok(())
    }
}

impl VerifiedHeaderStore for InMemoryVerifiedHeaderStore {
    fn window(&self) -> ServiceResult<Option<HeaderStageWindow>> {
        Ok(self.state.read().window.clone())
    }

    fn header(&self, height: u32) -> ServiceResult<Option<Header>> {
        Ok(self.state.read().headers.get(&height).cloned())
    }

    fn begin_window(
        &self,
        base_height: u32,
        target_height: u32,
    ) -> ServiceResult<HeaderStageWindow> {
        self.begin_or_reset_window(base_height, target_height, false)
    }

    fn reset_window(
        &self,
        base_height: u32,
        target_height: u32,
    ) -> ServiceResult<HeaderStageWindow> {
        self.begin_or_reset_window(base_height, target_height, true)
    }

    fn commit_verified_headers(&self, headers: &[Header]) -> ServiceResult<SyncStageCheckpoint> {
        let mut state = self.state.write();
        let window = state
            .window
            .clone()
            .ok_or_else(|| ServiceError::invalid_state("no active verified-header window"))?;
        let checkpoint = state
            .checkpoints
            .get(&SyncStageKind::Headers)
            .cloned()
            .ok_or_else(|| {
                ServiceError::invalid_state("missing Headers checkpoint for active window")
            })?;
        let previous_header = if checkpoint.height > window.base_height {
            Some(
                state
                    .headers
                    .get(&checkpoint.height)
                    .cloned()
                    .ok_or_else(|| {
                        ServiceError::invalid_state(format!(
                            "missing staged header {} for Headers checkpoint",
                            checkpoint.height
                        ))
                    })?,
            )
        } else {
            None
        };
        let prepared =
            prepare_verified_commit(&window, &checkpoint, previous_header.as_ref(), headers)?;

        for staged in &prepared.headers {
            state
                .headers
                .insert(staged.height, staged.stored.header().clone());
        }
        state
            .checkpoints
            .insert(SyncStageKind::Headers, prepared.checkpoint.clone());
        state.window = Some(HeaderStageWindow {
            target_hash: prepared.target_hash,
            ..window
        });
        Ok(prepared.checkpoint)
    }

    fn finish_window(&self, canonical_height: u32) -> ServiceResult<SyncStageCheckpoint> {
        let mut state = self.state.write();
        let window = state
            .window
            .clone()
            .ok_or_else(|| ServiceError::invalid_state("no active verified-header window"))?;
        if canonical_height < window.target_height {
            return Err(ServiceError::invalid_input(format!(
                "canonical height {canonical_height} is below verified-header target {}",
                window.target_height
            )));
        }
        let checkpoint = state
            .checkpoints
            .get(&SyncStageKind::Headers)
            .cloned()
            .unwrap_or_else(|| {
                SyncStageCheckpoint::new(SyncStageKind::Headers, window.base_height)
            });
        validate_completed_window(&window, &checkpoint)?;
        let finished = finished_checkpoint(&checkpoint, canonical_height);
        clear_in_memory_window(&mut state, &window);
        state
            .checkpoints
            .insert(SyncStageKind::Headers, finished.clone());
        Ok(finished)
    }

    fn discard_window(&self, canonical_height: u32) -> ServiceResult<SyncStageCheckpoint> {
        let mut state = self.state.write();
        let checkpoint = state
            .checkpoints
            .get(&SyncStageKind::Headers)
            .cloned()
            .unwrap_or_else(|| SyncStageCheckpoint::new(SyncStageKind::Headers, canonical_height));
        if let Some(window) = state.window.clone() {
            clear_in_memory_window(&mut state, &window);
        }
        let discarded = finished_checkpoint(&checkpoint, canonical_height);
        state
            .checkpoints
            .insert(SyncStageKind::Headers, discarded.clone());
        Ok(discarded)
    }
}

impl InMemoryVerifiedHeaderStore {
    fn begin_or_reset_window(
        &self,
        base_height: u32,
        target_height: u32,
        reset: bool,
    ) -> ServiceResult<HeaderStageWindow> {
        validate_window_bounds(base_height, target_height)?;
        let mut state = self.state.write();
        if let Some(active) = state.window.clone() {
            if !reset {
                if active.base_height == base_height && active.target_height == target_height {
                    return Ok(active);
                }
                return Err(active_window_error(&active));
            }
            clear_in_memory_window(&mut state, &active);
        }

        let window = HeaderStageWindow {
            base_height,
            target_height,
            target_hash: None,
        };
        state.checkpoints.insert(
            SyncStageKind::Headers,
            SyncStageCheckpoint::new(SyncStageKind::Headers, base_height),
        );
        state.window = Some(window.clone());
        Ok(window)
    }
}

fn clear_in_memory_window(state: &mut InMemoryVerifiedHeaderState, window: &HeaderStageWindow) {
    let heights: Vec<_> = state
        .headers
        .range(window.base_height.saturating_add(1)..=window.target_height)
        .map(|(height, _)| *height)
        .collect();
    for height in heights {
        state.headers.remove(&height);
    }
    state.window = None;
}

/// Store-backed verified-header store over a concrete backend handle.
#[derive(Debug)]
pub struct StoreVerifiedHeaderStore<S: TransactionalStore> {
    store: S,
}

impl<S: TransactionalStore> StoreVerifiedHeaderStore<S> {
    /// Creates a verified-header store over one concrete backend handle.
    #[must_use]
    pub const fn new(store: S) -> Self {
        Self { store }
    }

    /// Returns the underlying store handle.
    #[must_use]
    pub const fn store(&self) -> &S {
        &self.store
    }
}

impl<S: TransactionalStore> SyncStageCheckpointStore for StoreVerifiedHeaderStore<S> {
    fn checkpoint(&self, stage: SyncStageKind) -> ServiceResult<Option<SyncStageCheckpoint>> {
        read_checkpoint(&self.store, stage)
    }

    fn put_checkpoint(&self, checkpoint: SyncStageCheckpoint) -> ServiceResult<()> {
        write_checkpoint(&self.store, checkpoint)
    }
}

impl<S: TransactionalStore> VerifiedHeaderStore for StoreVerifiedHeaderStore<S> {
    fn window(&self) -> ServiceResult<Option<HeaderStageWindow>> {
        read_window(&self.store)
    }

    fn header(&self, height: u32) -> ServiceResult<Option<Header>> {
        read_verified_header(&self.store, height)
    }

    fn begin_window(
        &self,
        base_height: u32,
        target_height: u32,
    ) -> ServiceResult<HeaderStageWindow> {
        begin_or_reset_store_window(&self.store, base_height, target_height, false)
    }

    fn reset_window(
        &self,
        base_height: u32,
        target_height: u32,
    ) -> ServiceResult<HeaderStageWindow> {
        begin_or_reset_store_window(&self.store, base_height, target_height, true)
    }

    fn commit_verified_headers(&self, headers: &[Header]) -> ServiceResult<SyncStageCheckpoint> {
        commit_store_verified_headers(&self.store, headers)
    }

    fn finish_window(&self, canonical_height: u32) -> ServiceResult<SyncStageCheckpoint> {
        finish_store_window(&self.store, canonical_height)
    }

    fn discard_window(&self, canonical_height: u32) -> ServiceResult<SyncStageCheckpoint> {
        discard_store_window(&self.store, canonical_height)
    }
}

/// Store-backed verified-header store over a shared backend handle.
#[derive(Debug)]
pub struct SharedStoreVerifiedHeaderStore<S: TransactionalStore = MemoryStore> {
    store: Arc<S>,
}

impl<S: TransactionalStore> SharedStoreVerifiedHeaderStore<S> {
    /// Creates a verified-header store over a shared storage backend.
    #[must_use]
    pub const fn new(store: Arc<S>) -> Self {
        Self { store }
    }

    /// Returns the shared store handle.
    #[must_use]
    pub fn store(&self) -> Arc<S> {
        Arc::clone(&self.store)
    }
}

impl<S: TransactionalStore> SyncStageCheckpointStore for SharedStoreVerifiedHeaderStore<S> {
    fn checkpoint(&self, stage: SyncStageKind) -> ServiceResult<Option<SyncStageCheckpoint>> {
        read_checkpoint(self.store.as_ref(), stage)
    }

    fn put_checkpoint(&self, checkpoint: SyncStageCheckpoint) -> ServiceResult<()> {
        write_checkpoint(self.store.as_ref(), checkpoint)
    }
}

impl<S: TransactionalStore> VerifiedHeaderStore for SharedStoreVerifiedHeaderStore<S> {
    fn window(&self) -> ServiceResult<Option<HeaderStageWindow>> {
        read_window(self.store.as_ref())
    }

    fn header(&self, height: u32) -> ServiceResult<Option<Header>> {
        read_verified_header(self.store.as_ref(), height)
    }

    fn begin_window(
        &self,
        base_height: u32,
        target_height: u32,
    ) -> ServiceResult<HeaderStageWindow> {
        begin_or_reset_store_window(self.store.as_ref(), base_height, target_height, false)
    }

    fn reset_window(
        &self,
        base_height: u32,
        target_height: u32,
    ) -> ServiceResult<HeaderStageWindow> {
        begin_or_reset_store_window(self.store.as_ref(), base_height, target_height, true)
    }

    fn commit_verified_headers(&self, headers: &[Header]) -> ServiceResult<SyncStageCheckpoint> {
        commit_store_verified_headers(self.store.as_ref(), headers)
    }

    fn finish_window(&self, canonical_height: u32) -> ServiceResult<SyncStageCheckpoint> {
        finish_store_window(self.store.as_ref(), canonical_height)
    }

    fn discard_window(&self, canonical_height: u32) -> ServiceResult<SyncStageCheckpoint> {
        discard_store_window(self.store.as_ref(), canonical_height)
    }
}

struct PreparedStagedHeader {
    height: u32,
    stored: StoredVerifiedHeader,
}

struct PreparedCommit {
    headers: Vec<PreparedStagedHeader>,
    checkpoint: SyncStageCheckpoint,
    target_hash: Option<UInt256>,
}

fn validate_window_bounds(base_height: u32, target_height: u32) -> ServiceResult<()> {
    if target_height <= base_height {
        return Err(ServiceError::invalid_input(format!(
            "verified-header target {target_height} must be above canonical height {base_height}"
        )));
    }
    if target_height.saturating_sub(base_height) > MAX_VERIFIED_HEADER_WINDOW {
        return Err(ServiceError::invalid_input(format!(
            "verified-header window exceeds the {MAX_VERIFIED_HEADER_WINDOW}-header limit"
        )));
    }
    Ok(())
}

fn active_window_error(window: &HeaderStageWindow) -> ServiceError {
    ServiceError::invalid_state(format!(
        "active header stage window is fixed at {}..={}; reset it before changing the target",
        window.base_height.saturating_add(1),
        window.target_height
    ))
}

fn begin_or_reset_store_window<S: TransactionalStore>(
    store: &S,
    base_height: u32,
    target_height: u32,
    reset: bool,
) -> ServiceResult<HeaderStageWindow> {
    validate_window_bounds(base_height, target_height)?;
    if let Some(active) = read_window(store)? {
        if !reset {
            if active.base_height == base_height && active.target_height == target_height {
                return Ok(active);
            }
            return Err(active_window_error(&active));
        }
    }

    let mut maintenance = StoreMaintenanceBatch::new();
    if let Some(active) = read_window(store)? {
        delete_window_range(&mut maintenance, &active)?;
    }
    let checkpoint = SyncStageCheckpoint::new(SyncStageKind::Headers, base_height);
    maintenance
        .put::<SyncCheckpointTable>(&SyncStageKind::Headers, &checkpoint)
        .map_err(|error| table_read_error("encode Headers checkpoint", error))?;
    let window = HeaderStageWindow {
        base_height,
        target_height,
        target_hash: None,
    };
    maintenance
        .put::<VerifiedHeaderWindowTable>(&(), &window)
        .map_err(|error| table_read_error("encode verified-header window", error))?;
    maintenance
        .delete::<VerifiedHeaderTargetHashTable>(&())
        .map_err(|error| table_read_error("encode verified-header target deletion", error))?;
    commit_maintenance(store, &maintenance, "begin verified-header window")?;
    Ok(window)
}

fn commit_store_verified_headers<S: TransactionalStore>(
    store: &S,
    headers: &[Header],
) -> ServiceResult<SyncStageCheckpoint> {
    let window = read_window(store)?
        .ok_or_else(|| ServiceError::invalid_state("no active verified-header window"))?;
    let checkpoint = read_checkpoint(store, SyncStageKind::Headers)?.ok_or_else(|| {
        ServiceError::invalid_state("missing Headers checkpoint for active window")
    })?;
    let previous_header = if checkpoint.height > window.base_height {
        Some(
            read_verified_header(store, checkpoint.height)?.ok_or_else(|| {
                ServiceError::invalid_state(format!(
                    "missing staged header {} for Headers checkpoint",
                    checkpoint.height
                ))
            })?,
        )
    } else {
        None
    };
    let prepared =
        prepare_verified_commit(&window, &checkpoint, previous_header.as_ref(), headers)?;

    let mut maintenance = StoreMaintenanceBatch::new();
    maintenance
        .put::<SyncCheckpointTable>(&SyncStageKind::Headers, &prepared.checkpoint)
        .map_err(|error| table_read_error("encode Headers checkpoint", error))?;
    if let Some(target_hash) = prepared.target_hash {
        maintenance
            .put::<VerifiedHeaderTargetHashTable>(&(), &target_hash)
            .map_err(|error| table_read_error("encode verified-header target hash", error))?;
    } else {
        maintenance
            .delete::<VerifiedHeaderTargetHashTable>(&())
            .map_err(|error| table_read_error("encode verified-header target deletion", error))?;
    }
    for staged in prepared.headers {
        maintenance
            .put::<VerifiedHeaderTable>(&staged.height, &staged.stored)
            .map_err(|error| table_read_error("encode verified header", error))?;
    }
    commit_maintenance(store, &maintenance, "commit verified headers")?;
    Ok(prepared.checkpoint)
}

fn finish_store_window<S: TransactionalStore>(
    store: &S,
    canonical_height: u32,
) -> ServiceResult<SyncStageCheckpoint> {
    let window = read_window(store)?
        .ok_or_else(|| ServiceError::invalid_state("no active verified-header window"))?;
    if canonical_height < window.target_height {
        return Err(ServiceError::invalid_input(format!(
            "canonical height {canonical_height} is below verified-header target {}",
            window.target_height
        )));
    }
    let checkpoint = read_checkpoint(store, SyncStageKind::Headers)?
        .unwrap_or_else(|| SyncStageCheckpoint::new(SyncStageKind::Headers, window.base_height));
    validate_completed_window(&window, &checkpoint)?;
    let finished = finished_checkpoint(&checkpoint, canonical_height);

    let mut maintenance = StoreMaintenanceBatch::new();
    delete_window_range(&mut maintenance, &window)?;
    maintenance
        .put::<SyncCheckpointTable>(&SyncStageKind::Headers, &finished)
        .map_err(|error| table_read_error("encode finished Headers checkpoint", error))?;
    commit_maintenance(store, &maintenance, "finish verified-header window")?;
    Ok(finished)
}

fn discard_store_window<S: TransactionalStore>(
    store: &S,
    canonical_height: u32,
) -> ServiceResult<SyncStageCheckpoint> {
    let window = read_window(store)?;
    let checkpoint = match read_checkpoint(store, SyncStageKind::Headers) {
        Ok(Some(checkpoint)) => checkpoint,
        Ok(None) | Err(ServiceError::InvalidState(_)) => {
            SyncStageCheckpoint::new(SyncStageKind::Headers, canonical_height)
        }
        Err(error) => return Err(error),
    };
    let discarded = finished_checkpoint(&checkpoint, canonical_height);

    let mut maintenance = StoreMaintenanceBatch::new();
    if let Some(window) = window {
        delete_window_range(&mut maintenance, &window)?;
    }
    maintenance
        .put::<SyncCheckpointTable>(&SyncStageKind::Headers, &discarded)
        .map_err(|error| table_read_error("encode discarded Headers checkpoint", error))?;
    commit_maintenance(store, &maintenance, "discard verified-header window")?;
    Ok(discarded)
}

fn finished_checkpoint(
    checkpoint: &SyncStageCheckpoint,
    canonical_height: u32,
) -> SyncStageCheckpoint {
    SyncStageCheckpoint::new(SyncStageKind::Headers, canonical_height)
        .with_counters(checkpoint.processed_blocks, checkpoint.changed_bytes)
}

fn validate_completed_window(
    window: &HeaderStageWindow,
    checkpoint: &SyncStageCheckpoint,
) -> ServiceResult<()> {
    if checkpoint.stage != SyncStageKind::Headers
        || checkpoint.height != window.target_height
        || window.target_hash.is_none()
    {
        return Err(ServiceError::invalid_state(format!(
            "verified-header window {}..={} is incomplete at checkpoint {}",
            window.base_height.saturating_add(1),
            window.target_height,
            checkpoint.height
        )));
    }
    Ok(())
}

fn prepare_verified_commit(
    window: &HeaderStageWindow,
    checkpoint: &SyncStageCheckpoint,
    previous_header: Option<&Header>,
    headers: &[Header],
) -> ServiceResult<PreparedCommit> {
    if checkpoint.stage != SyncStageKind::Headers {
        return Err(ServiceError::invalid_state(format!(
            "verified-header window requires a Headers checkpoint, found {}",
            checkpoint.stage.as_str()
        )));
    }
    if checkpoint.height < window.base_height || checkpoint.height > window.target_height {
        return Err(ServiceError::invalid_state(format!(
            "Headers checkpoint {} is outside the active verified-header window {}..={}",
            checkpoint.height, window.base_height, window.target_height
        )));
    }
    if headers.is_empty() {
        return Err(ServiceError::invalid_input(
            "verified-header commit requires a nonempty contiguous prefix",
        ));
    }
    if checkpoint.height >= window.target_height {
        return Err(ServiceError::invalid_state(format!(
            "verified-header window already reached target {}",
            window.target_height
        )));
    }

    let mut expected_index = checkpoint.height.checked_add(1).ok_or_else(|| {
        ServiceError::invalid_state("Headers checkpoint overflowed while staging headers")
    })?;
    let mut expected_prev_hash = if checkpoint.height > window.base_height {
        Some(hash_header(previous_header.ok_or_else(|| {
            ServiceError::invalid_state(format!(
                "missing staged header {} for Headers checkpoint",
                checkpoint.height
            ))
        })?)?)
    } else {
        None
    };

    let mut staged_headers = Vec::with_capacity(headers.len());
    let mut changed_bytes = 0u64;
    let mut target_hash = None;

    for header in headers {
        if header.index() != expected_index {
            return Err(ServiceError::invalid_input(format!(
                "expected header index {expected_index}, got {}",
                header.index()
            )));
        }
        if header.index() > window.target_height {
            return Err(ServiceError::invalid_input(format!(
                "verified-header batch exceeds target {} at header {}",
                window.target_height,
                header.index()
            )));
        }
        if let Some(prev_hash) = expected_prev_hash {
            if header.prev_hash() != &prev_hash {
                return Err(ServiceError::invalid_input(format!(
                    "prev-hash linkage mismatch at header {}",
                    header.index()
                )));
            }
        }

        let bytes = serialize_header(header)?;
        changed_bytes = changed_bytes
            .checked_add(u64::try_from(bytes.len()).map_err(|_| {
                ServiceError::invalid_state("verified-header byte count overflowed u64")
            })?)
            .ok_or_else(|| {
                ServiceError::invalid_state("verified-header changed-byte counter overflowed")
            })?;
        let header_hash = hash_header(header)?;
        if header.index() == window.target_height {
            target_hash = Some(header_hash);
        }
        staged_headers.push(PreparedStagedHeader {
            height: header.index(),
            stored: StoredVerifiedHeader::new(header.clone(), bytes),
        });
        expected_index = expected_index
            .checked_add(1)
            .ok_or_else(|| ServiceError::invalid_state("header index overflow while staging"))?;
        expected_prev_hash = Some(header_hash);
    }

    let last_height = staged_headers
        .last()
        .map(|header| header.height)
        .ok_or_else(|| {
            ServiceError::invalid_input(
                "verified-header commit requires a nonempty contiguous prefix",
            )
        })?;
    Ok(PreparedCommit {
        headers: staged_headers,
        checkpoint: SyncStageCheckpoint::new(SyncStageKind::Headers, last_height).with_counters(
            checkpoint
                .processed_blocks
                .checked_add(u64::try_from(headers.len()).map_err(|_| {
                    ServiceError::invalid_state("verified-header block counter overflowed u64")
                })?)
                .ok_or_else(|| {
                    ServiceError::invalid_state(
                        "verified-header processed-block counter overflowed",
                    )
                })?,
            checkpoint
                .changed_bytes
                .checked_add(changed_bytes)
                .ok_or_else(|| {
                    ServiceError::invalid_state("verified-header changed-byte counter overflowed")
                })?,
        ),
        target_hash,
    })
}

fn serialize_header(header: &Header) -> ServiceResult<Vec<u8>> {
    header.try_to_bytes().map_err(|error| {
        ServiceError::invalid_input(format!(
            "serialize verified header {}: {error}",
            header.index()
        ))
    })
}

fn hash_header(header: &Header) -> ServiceResult<UInt256> {
    header.try_hash().map_err(|error| {
        ServiceError::invalid_input(format!("hash verified header {}: {error}", header.index()))
    })
}

fn read_window<S: TransactionalStore>(store: &S) -> ServiceResult<Option<HeaderStageWindow>> {
    let window = store
        .table_get::<VerifiedHeaderWindowTable>(&())
        .map_err(|error| table_read_error("read verified-header window", error))?;
    let target_hash = store
        .table_get::<VerifiedHeaderTargetHashTable>(&())
        .map_err(|error| table_read_error("read verified-header target hash", error))?;

    let Some(mut window) = window else {
        if target_hash.is_some() {
            return Err(ServiceError::invalid_state(
                "verified-header target hash exists without an active window",
            ));
        }
        return Ok(None);
    };
    window.target_hash = target_hash;
    Ok(Some(window))
}

fn read_verified_header<S: TransactionalStore>(
    store: &S,
    height: u32,
) -> ServiceResult<Option<Header>> {
    store
        .table_get::<VerifiedHeaderTable>(&height)
        .map(|header| header.map(StoredVerifiedHeader::into_header))
        .map_err(|error| table_read_error("read verified header", error))
}

fn delete_window_range(
    maintenance: &mut StoreMaintenanceBatch,
    window: &HeaderStageWindow,
) -> ServiceResult<()> {
    for height in window.base_height.saturating_add(1)..=window.target_height {
        maintenance
            .delete::<VerifiedHeaderTable>(&height)
            .map_err(|error| table_read_error("encode verified-header deletion", error))?;
    }
    maintenance
        .delete::<VerifiedHeaderWindowTable>(&())
        .map_err(|error| table_read_error("encode verified-header window deletion", error))?;
    maintenance
        .delete::<VerifiedHeaderTargetHashTable>(&())
        .map_err(|error| table_read_error("encode verified-header target deletion", error))?;
    Ok(())
}
