//! # neo-node::node::append_shadow
//!
//! Phase 1 append-frame shadow dual-write for the coordinated StateService
//! commit.
//!
//! When `[storage.append_shadow].enabled` is set, every coordinated commit
//! also feeds the StateService overlay entries (MPT node keys `0xf0 ||
//! node_hash` plus metadata records) to a [`ShadowPackWriter`] in its own
//! directory and persists a pack high-water record into the MDBX maintenance
//! table inside the same canonical transaction. MDBX remains authoritative
//! for every row: a shadow failure is logged and counted by the storage
//! backend and the canonical commit continues with a durable degraded marker.
//! That marker prevents a later process from resuming after a missing window.
//!
//! ## Boundary
//!
//! This module belongs to `neo-node`. It wires configuration to the
//! `neo-state-packs` shadow adapter; it defines no pack format or storage
//! semantics (those live in `neo-state-packs`).
//!
//! ## Contents
//!
//! - Configuration-scoped writer startup and marker reconciliation.
//! - Pre-marker durable frame preparation and post-commit activation.
//! - State-root span extraction for bounded verification metadata.

use std::path::Path;
use std::sync::Arc;

use anyhow::{Context, ensure};
use neo_state_packs::shadow::{
    PreparedShadowFrame, SHADOW_DEGRADED_KEY, SHADOW_HIGH_WATER_KEY, ShadowDegradedRecord,
    ShadowHighWaterRecord, ShadowPackWriter,
};
use neo_state_packs::{PackFrameContext, PackStoreConfig};
use neo_state_service::keys::{STATE_ROOT_PREFIX, state_root_index};
use neo_state_service::{
    StateRootRecordError, decode_local_state_root_record, read_current_local_root_from,
    read_local_state_root,
};
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::{
    ShadowCommitMarker, ShadowCommitOutcome, ShadowOverlayEntries, Store, TransactionalStore,
};
use parking_lot::Mutex;
use tracing::{info, warn};

use super::config::{AppendShadowSection, NodeConfig, network_scoped_path};

/// Shared shadow dual-write handle passed to the coordinated commit hooks.
///
/// The writer sits behind a mutex (the canonical commit serializes StateService
/// publication, so contention is minimal). After a shadow failure the writer is
/// dropped: torn bytes from the failed frame remain unreachable, while the
/// canonical transaction records a degraded marker. Restart keeps the shadow
/// disabled until an explicit rebuild or reseed removes that marker.
pub(in crate::node) struct AppendShadow {
    state: Mutex<AppendShadowState>,
}
struct AppendShadowState {
    writer: Option<ShadowPackWriter>,
    canonical_tip: Option<StateTip>,
    pending: Option<PendingShadowCommit>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StateTip {
    index: u32,
    root: [u8; 32],
}

#[derive(Debug)]
struct PendingShadowCommit {
    frame: Option<PreparedShadowFrame>,
    tip: StateTip,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct StateRootSpan {
    block_start: u32,
    block_end: u32,
    resulting_root: [u8; 32],
}

impl AppendShadow {
    /// Shadow hook invoked inside the canonical transaction: mirrors the
    /// window's StateService entries and returns the high-water marker row
    /// for the maintenance table. Errors bubble up to the MDBX commit layer,
    /// which logs and counts them and commits a degraded marker.
    pub(in crate::node) fn mirror_window(
        &self,
        entries: ShadowOverlayEntries,
    ) -> ShadowCommitOutcome {
        let span = match state_root_span(&entries) {
            Ok(Some(span)) => span,
            Ok(None) => {
                return degraded_outcome(
                    None,
                    None,
                    "append shadow window has no StateService root record".to_owned(),
                );
            }
            Err(error) => {
                return degraded_outcome(
                    None,
                    None,
                    format!("append shadow StateService root metadata is invalid: {error:#}"),
                );
            }
        };
        let mut state = self.state.lock();
        if state.pending.is_some() {
            state.pending = None;
            state.writer = None;
            return degraded_outcome(
                Some(span.block_end),
                Some(span.resulting_root),
                "append shadow already has a frame awaiting canonical publication".to_owned(),
            );
        }
        if state.writer.is_none() {
            return degraded_outcome(
                Some(span.block_end),
                Some(span.resulting_root),
                "append shadow writer is disabled after an earlier failure".to_owned(),
            );
        };
        let (expected_start, previous_root) = match state.canonical_tip {
            Some(canonical_tip) => match canonical_tip.index.checked_add(1) {
                Some(expected_start) => (expected_start, canonical_tip.root),
                None => {
                    state.writer = None;
                    return degraded_outcome(
                        Some(span.block_end),
                        Some(span.resulting_root),
                        "append shadow canonical StateService tip cannot advance past u32::MAX"
                            .to_owned(),
                    );
                }
            },
            None => (0, [0; 32]),
        };
        if span.block_start != expected_start {
            state.writer = None;
            return degraded_outcome(
                Some(span.block_end),
                Some(span.resulting_root),
                format!(
                    "append shadow block window starts at {}, expected {} after canonical tip {:?}",
                    span.block_start,
                    expected_start,
                    state.canonical_tip.map(|tip| tip.index)
                ),
            );
        }
        let context = PackFrameContext {
            block_start: span.block_start,
            block_end: span.block_end,
            previous_root,
            resulting_root: span.resulting_root,
        };
        let prepared = match state
            .writer
            .as_mut()
            .expect("shadow writer checked above")
            .prepare_state_overlay(context, entries)
        {
            Ok(receipt) => receipt,
            Err(error) => {
                let message = format!("shadow pack prepare failed: {error:#}");
                state.writer = None;
                return degraded_outcome(Some(span.block_end), Some(span.resulting_root), message);
            }
        };
        state.pending = Some(PendingShadowCommit {
            frame: prepared,
            tip: StateTip {
                index: span.block_end,
                root: span.resulting_root,
            },
        });
        let Some(prepared) = prepared else {
            return ShadowCommitOutcome::Unchanged;
        };
        let receipt = prepared.receipt();
        let record = ShadowHighWaterRecord::new(&receipt);
        ShadowCommitOutcome::Prepared(ShadowCommitMarker {
            key: SHADOW_HIGH_WATER_KEY.to_vec(),
            value: record.encode(),
        })
    }

    /// Activates the prepared frame after the canonical MDBX marker commits.
    pub(in crate::node) fn canonical_commit_succeeded(&self) {
        let mut state = self.state.lock();
        let Some(pending) = state.pending.take() else {
            return;
        };
        state.canonical_tip = Some(pending.tip);
        let Some(prepared) = pending.frame else {
            return;
        };
        let Some(writer) = state.writer.as_mut() else {
            return;
        };
        if let Err(error) = writer.activate_prepared(prepared) {
            warn!(
                target: "neo::append_shadow",
                error = %format_args!("{error:#}"),
                "canonical marker committed but shadow index activation failed; startup recovery will rebuild from the marker"
            );
            state.writer = None;
        }
    }

    /// Drops the writer after a failed canonical transaction. The durable
    /// prepared suffix remains invisible and startup truncates it to the last
    /// committed marker.
    pub(in crate::node) fn canonical_commit_failed(&self) {
        let mut state = self.state.lock();
        state.pending = None;
        state.writer = None;
    }
}

/// Opens the configured shadow writer, or returns `None` when the shadow is
/// disabled. An open failure is logged and disables the shadow instead of
/// aborting startup: the shadow is verification tooling and must never gate
/// the authoritative store.
pub(in crate::node) fn open_append_shadow(
    config: &NodeConfig,
    network: u32,
    canonical_store: &RuntimeStore,
) -> Option<Arc<AppendShadow>> {
    let section = &config.storage.append_shadow;
    if !section.enabled {
        return None;
    }
    let path = section
        .path
        .as_deref()
        .map(|path| network_scoped_path(path, network))?;
    let (high_water, degraded) = match load_recovery_markers(canonical_store) {
        Ok(markers) => markers,
        Err(error) => {
            warn!(
                target: "neo::append_shadow",
                path = %path.display(),
                error = %format_args!("{error:#}"),
                "append shadow high-water marker is unavailable; continuing without the shadow dual-write"
            );
            return None;
        }
    };
    if let Some(degraded) = degraded {
        warn!(
            target: "neo::append_shadow",
            path = %path.display(),
            block_index = ?degraded.block_index,
            state_root = ?degraded.state_root.map(hex::encode),
            "append shadow history is marked incomplete; rebuild or reseed it before removing the degraded marker"
        );
        return None;
    }
    let canonical_tip = match load_canonical_tip(canonical_store, high_water.as_ref()) {
        Ok(tip) => tip,
        Err(error) => {
            warn!(
                target: "neo::append_shadow",
                path = %path.display(),
                error = %format_args!("{error:#}"),
                "append shadow cannot bind its frame context to the canonical StateService tip"
            );
            return None;
        }
    };
    match open_writer(&path, section, high_water.as_ref()) {
        Ok(writer) => {
            info!(
                target: "neo::append_shadow",
                path = %path.display(),
                committed_epoch = high_water.as_ref().map(|record| record.epoch),
                "append-frame shadow dual-write enabled; MDBX remains authoritative"
            );
            Some(Arc::new(AppendShadow {
                state: Mutex::new(AppendShadowState {
                    writer: Some(writer),
                    canonical_tip,
                    pending: None,
                }),
            }))
        }
        Err(error) => {
            warn!(
                target: "neo::append_shadow",
                path = %path.display(),
                error = %format_args!("{error:#}"),
                "append shadow store failed to open; continuing without the shadow dual-write"
            );
            None
        }
    }
}

fn load_canonical_tip(
    canonical_store: &RuntimeStore,
    high_water: Option<&ShadowHighWaterRecord>,
) -> anyhow::Result<Option<StateTip>> {
    let state_service = canonical_store
        .open_coordinated_namespace(neo_state_service::MDBX_STATE_SERVICE_NAMESPACE)
        .context("open coordinated StateService namespace for append shadow tip")?;
    let snapshot = state_service.snapshot();
    let root = match read_current_local_root_from(snapshot.as_ref()) {
        Ok(root) => Some(root),
        Err(StateRootRecordError::MissingCurrentIndex) if high_water.is_none() => None,
        Err(error) => {
            return Err(error).context("read canonical StateService tip for append shadow");
        }
    };
    let canonical_tip = root.as_ref().map(|root| StateTip {
        index: root.index(),
        root: root.root_hash().to_array(),
    });
    if let Some(high_water) = high_water {
        let canonical_tip = canonical_tip
            .context("shadow high-water marker exists without a current local StateService root")?;
        validate_high_water_against_tip(high_water, canonical_tip, snapshot.as_ref())?;
    }
    Ok(canonical_tip)
}

fn validate_high_water_against_tip<R>(
    high_water: &ShadowHighWaterRecord,
    canonical_tip: StateTip,
    snapshot: &R,
) -> anyhow::Result<()>
where
    R: neo_storage::persistence::RawReadOnlyStore + ?Sized,
{
    ensure!(
        high_water.frame_context.block_end <= canonical_tip.index,
        "shadow frame ending at block {} is ahead of canonical tip {}",
        high_water.frame_context.block_end,
        canonical_tip.index
    );
    let ending_root = read_local_state_root(snapshot, high_water.frame_context.block_end)
        .context("read shadow frame ending root from canonical StateService history")?;
    ensure!(
        high_water.frame_context.resulting_root == ending_root.root_hash().to_array(),
        "shadow frame resulting root differs from canonical StateService history"
    );
    if high_water.frame_context.block_start == 0 {
        ensure!(
            high_water.frame_context.previous_root == [0; 32],
            "genesis shadow frame previous root is not the explicit pre-genesis zero root"
        );
    } else {
        let previous_index = high_water.frame_context.block_start - 1;
        let previous_root = read_local_state_root(snapshot, previous_index)
            .context("read shadow frame previous root from canonical StateService history")?;
        ensure!(
            high_water.frame_context.previous_root == previous_root.root_hash().to_array(),
            "shadow frame previous root differs from canonical StateService history"
        );
    }
    if high_water.frame_context.block_end == canonical_tip.index {
        ensure!(
            high_water.frame_context.resulting_root == canonical_tip.root,
            "shadow frame resulting root differs from canonical tip root"
        );
    }
    Ok(())
}

fn open_writer(
    path: &Path,
    section: &AppendShadowSection,
    high_water: Option<&ShadowHighWaterRecord>,
) -> anyhow::Result<ShadowPackWriter> {
    let pack_config = PackStoreConfig::default()
        .with_max_index_memory_bytes(section.max_index_memory_bytes())
        .context("validate append shadow pack-store configuration")?;
    ShadowPackWriter::open_or_create_at_high_water(path, pack_config, high_water)
        .with_context(|| format!("open append shadow store at {}", path.display()))
}

fn load_recovery_markers(
    canonical_store: &RuntimeStore,
) -> anyhow::Result<(Option<ShadowHighWaterRecord>, Option<ShadowDegradedRecord>)> {
    let state_service = canonical_store
        .open_coordinated_namespace(neo_state_service::MDBX_STATE_SERVICE_NAMESPACE)
        .context("open coordinated StateService namespace for append shadow recovery")?;
    let high_water = state_service
        .maintenance_metadata(SHADOW_HIGH_WATER_KEY)
        .context("read append shadow high-water marker")?
        .map(|bytes| {
            ShadowHighWaterRecord::decode(&bytes).context("decode append shadow high-water marker")
        })
        .transpose()?;
    let degraded = state_service
        .maintenance_metadata(SHADOW_DEGRADED_KEY)
        .context("read append shadow degraded marker")?
        .map(|bytes| {
            ShadowDegradedRecord::decode(&bytes).context("decode append shadow degraded marker")
        })
        .transpose()?;
    Ok((high_water, degraded))
}

fn degraded_outcome(
    block_index: Option<u32>,
    state_root: Option<[u8; 32]>,
    error: String,
) -> ShadowCommitOutcome {
    let record = ShadowDegradedRecord::new(block_index, state_root);
    ShadowCommitOutcome::Degraded {
        marker: ShadowCommitMarker {
            key: SHADOW_DEGRADED_KEY.to_vec(),
            value: record.encode().to_vec(),
        },
        error,
    }
}

/// Extracts the block-index span and newest state root from the mirrored
/// overlay entries. The visited channel carries the per-block state-root
/// records (`0x01 || index_be`, value `version(1) || index_le(4) ||
/// root(32) || witness_count(1)`); the cursor channel carries node entries.
fn state_root_span(
    entries: &[(Vec<u8>, Option<Vec<u8>>)],
) -> anyhow::Result<Option<StateRootSpan>> {
    let mut block_index_min = None;
    let mut block_index_max = None;
    let mut resulting_root = None;
    for (key, value) in entries {
        if key.first() != Some(&STATE_ROOT_PREFIX) {
            continue;
        }
        let index = state_root_index(key).context("StateService root record key is malformed")?;
        let value = value
            .as_deref()
            .with_context(|| format!("StateService root record {index} is deleted"))?;
        let root = decode_local_state_root_record(index, value)
            .with_context(|| format!("decode StateService root record {index}"))?;
        let is_min = block_index_min.is_none_or(|min: u32| index < min);
        if is_min {
            block_index_min = Some(index);
        }
        let is_max = block_index_max.is_none_or(|max: u32| index > max);
        if is_max {
            block_index_max = Some(index);
            resulting_root = Some(root.root_hash().to_array());
        }
    }
    Ok(match (block_index_min, block_index_max, resulting_root) {
        (Some(block_start), Some(block_end), Some(resulting_root)) => Some(StateRootSpan {
            block_start,
            block_end,
            resulting_root,
        }),
        (None, None, None) => None,
        _ => unreachable!("strict root decoding records complete span metadata"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::mdbx::MdbxStoreProvider;
    use neo_storage::persistence::providers::MemoryStore;
    use neo_storage::persistence::{StoreMaintenanceBatch, WriteStore, storage::StorageConfig};

    fn test_pack_config() -> PackStoreConfig {
        PackStoreConfig::default()
            .with_max_index_memory_bytes(64 * 1024 * 1024)
            .expect("valid append-shadow test configuration")
    }

    fn state_root_entry(index: u32, root_byte: u8) -> (Vec<u8>, Option<Vec<u8>>) {
        let key = neo_state_service::Keys::state_root(index);
        let mut value = vec![0x00];
        value.extend_from_slice(&index.to_le_bytes());
        value.extend_from_slice(&[root_byte; 32]);
        value.push(0x00);
        (key, Some(value))
    }

    #[test]
    fn state_root_span_tracks_min_max_and_newest_root() {
        let entries = vec![
            state_root_entry(42, 0xAA),
            (vec![0xf0, 0x11], Some(b"node".to_vec())),
            state_root_entry(40, 0xBB),
            state_root_entry(41, 0xCC),
        ];
        assert_eq!(
            state_root_span(&entries).expect("decode span"),
            Some(StateRootSpan {
                block_start: 40,
                block_end: 42,
                resulting_root: [0xAA; 32],
            })
        );
    }

    #[test]
    fn state_root_span_ignores_windows_without_root_records() {
        let entries: Vec<(Vec<u8>, Option<Vec<u8>>)> = vec![
            (vec![0xf0, 0x11], Some(b"node".to_vec())),
            (vec![0x02], Some(41u32.to_le_bytes().to_vec())),
        ];
        assert_eq!(state_root_span(&entries).expect("empty span"), None);
    }

    #[test]
    fn state_root_span_rejects_short_values() {
        let key = neo_state_service::Keys::state_root(7);
        let entries = vec![(key, Some(vec![0x00, 0x01]))];
        assert!(state_root_span(&entries).is_err());
    }

    #[test]
    fn state_root_span_rejects_noncanonical_record_metadata() {
        let mut wrong_version = state_root_entry(7, 0x77);
        wrong_version.1.as_mut().expect("state-root value")[0] = 1;
        assert!(state_root_span(&[wrong_version]).is_err());

        let mut wrong_index = state_root_entry(7, 0x77);
        wrong_index.1.as_mut().expect("state-root value")[1..5]
            .copy_from_slice(&8u32.to_le_bytes());
        assert!(state_root_span(&[wrong_index]).is_err());

        let malformed_key = vec![STATE_ROOT_PREFIX, 0, 0, 0];
        assert!(state_root_span(&[(malformed_key, Some(vec![0; 38]))]).is_err());
    }

    #[test]
    fn high_water_validation_binds_historical_previous_and_resulting_roots() {
        let mut roots = MemoryStore::new();
        for (index, root) in [(40, [0x10; 32]), (42, [0x22; 32])] {
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
        let record = ShadowHighWaterRecord {
            epoch: 0,
            frames_total: 1,
            segment_id: neo_state_packs::PackSegmentId::INITIAL,
            frame_end: neo_state_packs::PACK_SEGMENT_HEADER_LEN + 1,
            node_operations: 1,
            node_put_value_bytes: 1,
            frame_context: PackFrameContext::new(41, 42, [0x10; 32], [0x22; 32]),
            frame_sha256: [0x33; 32],
        };
        let tip = StateTip {
            index: 43,
            root: [0x22; 32],
        };
        validate_high_water_against_tip(&record, tip, &roots)
            .expect("valid historical frame continuity");

        roots
            .put(
                neo_state_service::Keys::state_root(42),
                neo_state_service::StateRoot::new_current(
                    42,
                    neo_primitives::UInt256::from([0x44; 32]),
                )
                .to_array(),
            )
            .expect("replace ending root");
        let error = validate_high_water_against_tip(&record, tip, &roots)
            .expect_err("ending-root mismatch must fail");
        assert!(error.to_string().contains("resulting root differs"));

        roots
            .put(
                neo_state_service::Keys::state_root(42),
                neo_state_service::StateRoot::new_current(
                    42,
                    neo_primitives::UInt256::from([0x22; 32]),
                )
                .to_array(),
            )
            .expect("restore ending root");
        roots
            .put(
                neo_state_service::Keys::state_root(40),
                neo_state_service::StateRoot::new_current(
                    40,
                    neo_primitives::UInt256::from([0x55; 32]),
                )
                .to_array(),
            )
            .expect("replace previous root");
        let error = validate_high_water_against_tip(&record, tip, &roots)
            .expect_err("previous-root mismatch must fail");
        assert!(error.to_string().contains("previous root differs"));
    }

    fn node_key(tag: u8) -> Vec<u8> {
        let mut key = vec![tag; 33];
        key[0] = 0xf0;
        key
    }

    fn append_shadow(writer: ShadowPackWriter) -> AppendShadow {
        append_shadow_at(writer, 40, [0x10; 32])
    }

    fn append_shadow_at(
        writer: ShadowPackWriter,
        canonical_index: u32,
        canonical_root: [u8; 32],
    ) -> AppendShadow {
        AppendShadow {
            state: Mutex::new(AppendShadowState {
                writer: Some(writer),
                canonical_tip: Some(StateTip {
                    index: canonical_index,
                    root: canonical_root,
                }),
                pending: None,
            }),
        }
    }

    fn fresh_append_shadow(writer: ShadowPackWriter) -> AppendShadow {
        AppendShadow {
            state: Mutex::new(AppendShadowState {
                writer: Some(writer),
                canonical_tip: None,
                pending: None,
            }),
        }
    }

    fn prepared_marker(outcome: ShadowCommitOutcome) -> ShadowCommitMarker {
        match outcome {
            ShadowCommitOutcome::Prepared(marker) => marker,
            other => panic!("expected prepared shadow marker, got {other:?}"),
        }
    }

    #[test]
    fn mirror_window_produces_marker_and_reopenable_exact_shadow() {
        let temp = tempfile::tempdir().expect("temp shadow root");
        let shadow_path = temp.path().join("shadow-packs");
        let writer = ShadowPackWriter::open_or_create(&shadow_path, test_pack_config())
            .expect("create shadow writer");
        let shadow = append_shadow(writer);

        let entries = vec![
            state_root_entry(41, 0x11),
            (node_key(1), Some(b"node-one".to_vec())),
            (node_key(2), Some(b"node-two".to_vec())),
            (vec![0x02], Some(41u32.to_le_bytes().to_vec())),
        ];
        let marker = prepared_marker(shadow.mirror_window(entries));
        assert_eq!(marker.key, SHADOW_HIGH_WATER_KEY);
        let record = ShadowHighWaterRecord::decode(&marker.value).expect("decode marker");
        assert_eq!(record.epoch, 0);
        assert_eq!(record.frames_total, 1);
        assert_eq!(record.node_operations, 2);
        assert_eq!(record.frame_context.block_start, 41);
        assert_eq!(record.frame_context.block_end, 41);
        assert_eq!(record.frame_context.previous_root, [0x10; 32]);
        assert_eq!(record.frame_context.resulting_root, [0x11; 32]);
        shadow.canonical_commit_succeeded();

        let entries = vec![
            state_root_entry(42, 0x22),
            (node_key(1), Some(b"node-one-new".to_vec())),
            (node_key(2), None),
        ];
        let marker = prepared_marker(shadow.mirror_window(entries));
        let record = ShadowHighWaterRecord::decode(&marker.value).expect("decode second marker");
        assert_eq!(record.epoch, 1);
        assert_eq!(record.frame_context.block_start, 42);
        assert_eq!(record.frame_context.block_end, 42);
        assert_eq!(record.frame_context.previous_root, [0x11; 32]);
        assert_eq!(record.frame_context.resulting_root, [0x22; 32]);
        shadow.canonical_commit_succeeded();
        drop(shadow);

        let store = neo_state_packs::PackStore::open(&shadow_path, test_pack_config())
            .expect("reopen shadow packs");
        assert_eq!(store.open_validation().frames, 2);
        let mut keys: Vec<[u8; 33]> = vec![
            node_key(1).try_into().expect("33-byte key"),
            node_key(2).try_into().expect("33-byte key"),
        ];
        keys.sort();
        let results = store.get_many_sorted(&keys).expect("read mirrored nodes");
        assert_eq!(
            results,
            vec![Some(b"node-one-new".to_vec()), None],
            "shadow entries must match the input overlay exactly"
        );
    }

    #[test]
    fn fresh_shadow_binds_genesis_to_the_explicit_pre_genesis_root() {
        let temp = tempfile::tempdir().expect("temp shadow root");
        let writer = ShadowPackWriter::open_or_create(temp.path(), test_pack_config())
            .expect("create shadow writer");
        let shadow = fresh_append_shadow(writer);
        let marker = prepared_marker(shadow.mirror_window(vec![
            state_root_entry(0, 0x11),
            (node_key(1), Some(b"genesis-node".to_vec())),
        ]));
        let record = ShadowHighWaterRecord::decode(&marker.value).expect("decode marker");
        assert_eq!(record.frame_context.block_start, 0);
        assert_eq!(record.frame_context.block_end, 0);
        assert_eq!(record.frame_context.previous_root, [0; 32]);
        assert_eq!(record.frame_context.resulting_root, [0x11; 32]);
        shadow.canonical_commit_succeeded();
        assert_eq!(
            shadow.state.lock().canonical_tip,
            Some(StateTip {
                index: 0,
                root: [0x11; 32],
            })
        );
    }

    #[test]
    fn mirror_window_without_node_entries_yields_no_marker() {
        let temp = tempfile::tempdir().expect("temp shadow root");
        let shadow_path = temp.path().join("shadow-packs");
        let writer = ShadowPackWriter::open_or_create(&shadow_path, test_pack_config())
            .expect("create shadow writer");
        let shadow = append_shadow_at(writer, 8, [0x88; 32]);
        let entries = vec![state_root_entry(9, 0x99)];
        assert_eq!(
            shadow.mirror_window(entries),
            ShadowCommitOutcome::Unchanged
        );
        shadow.canonical_commit_succeeded();
        assert_eq!(
            shadow.state.lock().canonical_tip,
            Some(StateTip {
                index: 9,
                root: [0x99; 32]
            })
        );
    }

    #[test]
    fn mirror_window_is_invisible_until_canonical_success() {
        let temp = tempfile::tempdir().expect("temp shadow root");
        let shadow_path = temp.path().join("shadow-packs");
        let writer = ShadowPackWriter::open_or_create(&shadow_path, test_pack_config())
            .expect("create shadow writer");
        let shadow = append_shadow(writer);
        let marker = prepared_marker(shadow.mirror_window(vec![
            state_root_entry(41, 0x11),
            (node_key(1), Some(b"node-one".to_vec())),
        ]));

        let manifests_before = std::fs::read_dir(&shadow_path)
            .expect("list shadow root before activation")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("manifest-") && name.ends_with(".man"))
            })
            .count();
        assert_eq!(manifests_before, 0);

        shadow.canonical_commit_succeeded();
        let manifests_after = std::fs::read_dir(&shadow_path)
            .expect("list shadow root after activation")
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_str()
                    .is_some_and(|name| name.starts_with("manifest-") && name.ends_with(".man"))
            })
            .count();
        assert_eq!(manifests_after, 1);

        let record = ShadowHighWaterRecord::decode(&marker.value).expect("decode marker");
        drop(shadow);
        let store = neo_state_packs::PackStore::open_at_commit_horizon(
            &shadow_path,
            test_pack_config(),
            Some(neo_state_packs::PackCommitHorizon {
                epoch: record.epoch,
                segment_id: record.segment_id,
                frame_end: record.frame_end,
                context: record.frame_context,
                frame_sha256: record.frame_sha256,
            }),
        )
        .expect("reopen activated shadow");
        assert_eq!(store.open_validation().frames, 1);
    }

    #[test]
    fn canonical_failure_leaves_only_an_uncommitted_orphan_suffix() {
        let temp = tempfile::tempdir().expect("temp shadow root");
        let shadow_path = temp.path().join("shadow-packs");
        let writer = ShadowPackWriter::open_or_create(&shadow_path, test_pack_config())
            .expect("create shadow writer");
        let shadow = append_shadow(writer);
        let first = prepared_marker(shadow.mirror_window(vec![
            state_root_entry(41, 0x11),
            (node_key(1), Some(b"committed".to_vec())),
        ]));
        let committed = ShadowHighWaterRecord::decode(&first.value).expect("decode first marker");
        shadow.canonical_commit_succeeded();

        let orphan = prepared_marker(shadow.mirror_window(vec![
            state_root_entry(42, 0x22),
            (node_key(1), Some(b"orphan".to_vec())),
        ]));
        assert_eq!(orphan.key, SHADOW_HIGH_WATER_KEY);
        shadow.canonical_commit_failed();
        drop(shadow);

        let store = neo_state_packs::PackStore::open_at_commit_horizon(
            &shadow_path,
            test_pack_config(),
            Some(neo_state_packs::PackCommitHorizon {
                epoch: committed.epoch,
                segment_id: committed.segment_id,
                frame_end: committed.frame_end,
                context: committed.frame_context,
                frame_sha256: committed.frame_sha256,
            }),
        )
        .expect("reopen at committed marker");
        let key: [u8; 33] = node_key(1).try_into().expect("node key");
        assert_eq!(
            store.get(&key).expect("read committed frame"),
            Some(b"committed".to_vec())
        );
        assert_eq!(store.open_validation().frames, 1);
    }

    #[test]
    fn unresolved_window_returns_a_durable_degraded_marker() {
        let temp = tempfile::tempdir().expect("temp shadow root");
        let shadow_path = temp.path().join("shadow-packs");
        let writer = ShadowPackWriter::open_or_create(&shadow_path, test_pack_config())
            .expect("create shadow writer");
        let shadow = append_shadow(writer);

        let first = shadow.mirror_window(vec![
            state_root_entry(41, 0x11),
            (node_key(1), Some(b"pending".to_vec())),
        ]);
        assert!(matches!(first, ShadowCommitOutcome::Prepared(_)));

        let degraded = shadow.mirror_window(vec![
            state_root_entry(42, 0x22),
            (node_key(1), Some(b"missed".to_vec())),
        ]);
        let ShadowCommitOutcome::Degraded { marker, .. } = degraded else {
            panic!("overlapping window must poison shadow continuity");
        };
        assert_eq!(marker.key, SHADOW_DEGRADED_KEY);
        assert_eq!(
            ShadowDegradedRecord::decode(&marker.value),
            Some(ShadowDegradedRecord::new(Some(42), Some([0x22; 32])))
        );

        let later = shadow.mirror_window(vec![
            state_root_entry(43, 0x33),
            (node_key(1), Some(b"later".to_vec())),
        ]);
        assert!(matches!(later, ShadowCommitOutcome::Degraded { .. }));
    }

    #[test]
    fn fresh_genesis_startup_allows_an_absent_current_root_pointer() {
        let temp = tempfile::tempdir().expect("temp MDBX root");
        let provider = MdbxStoreProvider::new(StorageConfig {
            path: temp.path().join("canonical"),
            mdbx_geometry_upper_bytes: Some(64 * 1024 * 1024),
            mdbx_geometry_growth_bytes: Some(4 * 1024 * 1024),
            ..StorageConfig::default()
        });
        let canonical = RuntimeStore::Mdbx(
            provider
                .get_mdbx_store(Path::new(""))
                .expect("open canonical MDBX"),
        );
        canonical
            .open_coordinated_namespace(neo_state_service::MDBX_STATE_SERVICE_NAMESPACE)
            .expect("create StateService namespace");

        let shadow_root = temp.path().join("shadow");
        let mut config = NodeConfig::default();
        config.storage.append_shadow.enabled = true;
        config.storage.append_shadow.path = Some(shadow_root.clone());
        config.storage.append_shadow.max_index_memory_mb = Some(64);

        assert!(
            open_append_shadow(&config, 0x334F_454E, &canonical).is_some(),
            "missing current root is the explicit fresh pre-genesis state"
        );
        assert!(shadow_root.exists());
    }

    #[test]
    fn durable_degraded_marker_prevents_shadow_restart() {
        let temp = tempfile::tempdir().expect("temp MDBX root");
        let provider = MdbxStoreProvider::new(StorageConfig {
            path: temp.path().join("canonical"),
            mdbx_geometry_upper_bytes: Some(64 * 1024 * 1024),
            mdbx_geometry_growth_bytes: Some(4 * 1024 * 1024),
            ..StorageConfig::default()
        });
        let canonical = RuntimeStore::Mdbx(
            provider
                .get_mdbx_store(Path::new(""))
                .expect("open canonical MDBX"),
        );
        let state_service = canonical
            .open_coordinated_namespace(neo_state_service::MDBX_STATE_SERVICE_NAMESPACE)
            .expect("open StateService namespace");
        let degraded = ShadowDegradedRecord::new(Some(42), Some([0x22; 32]));
        let mut maintenance = StoreMaintenanceBatch::new();
        maintenance.put_metadata(SHADOW_DEGRADED_KEY.to_vec(), degraded.encode().to_vec());
        state_service
            .commit_maintenance(&maintenance)
            .expect("commit degraded marker");

        let shadow_root = temp.path().join("shadow");
        let mut config = NodeConfig::default();
        config.storage.append_shadow.enabled = true;
        config.storage.append_shadow.path = Some(shadow_root.clone());
        config.storage.append_shadow.max_index_memory_mb = Some(64);

        assert!(
            open_append_shadow(&config, 0x334F_454E, &canonical).is_none(),
            "startup must not resume after a durably poisoned shadow window"
        );
        assert!(
            !shadow_root.exists(),
            "startup must reject the degraded history before opening pack bytes"
        );
    }
}
