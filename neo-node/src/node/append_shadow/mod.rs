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

use anyhow::Context;
use neo_state_packs::PackStoreConfig;
use neo_state_packs::shadow::{
    PreparedShadowFrame, SHADOW_DEGRADED_KEY, SHADOW_HIGH_WATER_KEY, ShadowDegradedRecord,
    ShadowHighWaterRecord, ShadowPackWriter,
};
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::{
    ShadowCommitMarker, ShadowCommitOutcome, ShadowOverlayEntries, TransactionalStore,
};
use parking_lot::Mutex;
use tracing::{info, warn};

use super::config::{AppendShadowSection, NodeConfig, network_scoped_path};

/// StateService state-root record key prefix (`0x01 || index_be`).
const STATE_ROOT_KEY_PREFIX: u8 = 0x01;
/// Serialized unsigned state-root record prefix length:
/// `version(1) + index_le(4) + root_hash(32)` — mirrors
/// `MptStore::encode_state_root_fields` in `neo-state-service`.
const STATE_ROOT_VALUE_ROOT_OFFSET: usize = 5;
const STATE_ROOT_VALUE_UNSIGNED_LEN: usize = 1 + 4 + 32;

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
    pending: Option<PreparedShadowFrame>,
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
        let (block_index_min, block_index_max, state_root) = state_root_span(&entries);
        let mut state = self.state.lock();
        if state.pending.is_some() {
            state.pending = None;
            state.writer = None;
            return degraded_outcome(
                block_index_max,
                state_root,
                "append shadow already has a frame awaiting canonical publication".to_owned(),
            );
        }
        if state.writer.is_none() {
            return degraded_outcome(
                block_index_max,
                state_root,
                "append shadow writer is disabled after an earlier failure".to_owned(),
            );
        };
        let prepared = match state
            .writer
            .as_mut()
            .expect("shadow writer checked above")
            .prepare_state_overlay(entries)
        {
            Ok(receipt) => receipt,
            Err(error) => {
                let message = format!("shadow pack prepare failed: {error:#}");
                state.writer = None;
                return degraded_outcome(block_index_max, state_root, message);
            }
        };
        let Some(prepared) = prepared else {
            return ShadowCommitOutcome::Unchanged;
        };
        let receipt = prepared.receipt();
        state.pending = Some(prepared);
        let record =
            ShadowHighWaterRecord::new(&receipt, block_index_min, block_index_max, state_root);
        ShadowCommitOutcome::Prepared(ShadowCommitMarker {
            key: SHADOW_HIGH_WATER_KEY.to_vec(),
            value: record.encode(),
        })
    }

    /// Activates the prepared frame after the canonical MDBX marker commits.
    pub(in crate::node) fn canonical_commit_succeeded(&self) {
        let mut state = self.state.lock();
        let Some(prepared) = state.pending.take() else {
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
pub(in crate::node) fn state_root_span(
    entries: &[(Vec<u8>, Option<Vec<u8>>)],
) -> (Option<u32>, Option<u32>, Option<[u8; 32]>) {
    let mut block_index_min = None;
    let mut block_index_max = None;
    let mut state_root = None;
    for (key, value) in entries {
        if key.len() != 5 || key.first() != Some(&STATE_ROOT_KEY_PREFIX) {
            continue;
        }
        let index = u32::from_be_bytes(key[1..5].try_into().expect("four-byte index"));
        let is_min = block_index_min.is_none_or(|min: u32| index < min);
        if is_min {
            block_index_min = Some(index);
        }
        let is_max = block_index_max.is_none_or(|max: u32| index > max);
        if is_max {
            block_index_max = Some(index);
            state_root = value.as_deref().and_then(|value| {
                if value.len() < STATE_ROOT_VALUE_UNSIGNED_LEN || value[0] != 0 {
                    return None;
                }
                let encoded_index = u32::from_le_bytes(
                    value[1..STATE_ROOT_VALUE_ROOT_OFFSET]
                        .try_into()
                        .expect("four-byte encoded state-root index"),
                );
                if encoded_index != index {
                    return None;
                }
                let mut root = [0u8; 32];
                root.copy_from_slice(
                    &value[STATE_ROOT_VALUE_ROOT_OFFSET..STATE_ROOT_VALUE_UNSIGNED_LEN],
                );
                Some(root)
            });
        }
    }
    (block_index_min, block_index_max, state_root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::mdbx::MdbxStoreProvider;
    use neo_storage::persistence::{StoreMaintenanceBatch, storage::StorageConfig};

    fn test_pack_config() -> PackStoreConfig {
        PackStoreConfig::default()
            .with_max_index_memory_bytes(64 * 1024 * 1024)
            .expect("valid append-shadow test configuration")
    }

    fn state_root_entry(index: u32, root_byte: u8) -> (Vec<u8>, Option<Vec<u8>>) {
        let mut key = vec![STATE_ROOT_KEY_PREFIX];
        key.extend_from_slice(&index.to_be_bytes());
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
        let (min, max, root) = state_root_span(&entries);
        assert_eq!(min, Some(40));
        assert_eq!(max, Some(42));
        assert_eq!(root, Some([0xAA; 32]));
    }

    #[test]
    fn state_root_span_ignores_windows_without_root_records() {
        let entries: Vec<(Vec<u8>, Option<Vec<u8>>)> = vec![
            (vec![0xf0, 0x11], Some(b"node".to_vec())),
            (vec![0x02], Some(41u32.to_le_bytes().to_vec())),
        ];
        assert_eq!(state_root_span(&entries), (None, None, None));
    }

    #[test]
    fn state_root_span_tolerates_short_values() {
        let mut key = vec![STATE_ROOT_KEY_PREFIX];
        key.extend_from_slice(&7u32.to_be_bytes());
        let entries = vec![(key, Some(vec![0x00, 0x01]))];
        let (min, max, root) = state_root_span(&entries);
        assert_eq!(min, Some(7));
        assert_eq!(max, Some(7));
        assert_eq!(root, None);
    }

    #[test]
    fn state_root_span_rejects_noncanonical_record_metadata() {
        let mut wrong_version = state_root_entry(7, 0x77);
        wrong_version.1.as_mut().expect("state-root value")[0] = 1;
        assert_eq!(state_root_span(&[wrong_version]), (Some(7), Some(7), None));

        let mut wrong_index = state_root_entry(7, 0x77);
        wrong_index.1.as_mut().expect("state-root value")[1..5]
            .copy_from_slice(&8u32.to_le_bytes());
        assert_eq!(state_root_span(&[wrong_index]), (Some(7), Some(7), None));
    }

    fn node_key(tag: u8) -> Vec<u8> {
        let mut key = vec![tag; 33];
        key[0] = 0xf0;
        key
    }

    fn append_shadow(writer: ShadowPackWriter) -> AppendShadow {
        AppendShadow {
            state: Mutex::new(AppendShadowState {
                writer: Some(writer),
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
        assert_eq!(record.block_index_min, Some(41));
        assert_eq!(record.block_index_max, Some(41));
        assert_eq!(record.state_root, Some([0x11; 32]));
        shadow.canonical_commit_succeeded();

        let entries = vec![
            state_root_entry(42, 0x22),
            (node_key(1), Some(b"node-one-new".to_vec())),
            (node_key(2), None),
        ];
        let marker = prepared_marker(shadow.mirror_window(entries));
        let record = ShadowHighWaterRecord::decode(&marker.value).expect("decode second marker");
        assert_eq!(record.epoch, 1);
        assert_eq!(record.block_index_max, Some(42));
        assert_eq!(record.state_root, Some([0x22; 32]));
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
    fn mirror_window_without_node_entries_yields_no_marker() {
        let temp = tempfile::tempdir().expect("temp shadow root");
        let shadow_path = temp.path().join("shadow-packs");
        let writer = ShadowPackWriter::open_or_create(&shadow_path, test_pack_config())
            .expect("create shadow writer");
        let shadow = append_shadow(writer);
        let entries = vec![state_root_entry(9, 0x99)];
        assert_eq!(
            shadow.mirror_window(entries),
            ShadowCommitOutcome::Unchanged
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
                payload_sha256: record.frame_payload_sha256,
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
                payload_sha256: committed.frame_payload_sha256,
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
