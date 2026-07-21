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
//! backend and the canonical commit continues without the marker.
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
    PreparedShadowFrame, SHADOW_HIGH_WATER_KEY, ShadowHighWaterRecord, ShadowPackWriter,
};
use neo_storage::persistence::providers::RuntimeStore;
use neo_storage::persistence::{ShadowCommitMarker, ShadowOverlayEntries, TransactionalStore};
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
/// dropped: torn bytes from the failed frame are truncated by the engine on the
/// next process open, and the MDBX layer has already logged and counted the
/// failure, so the node keeps importing with the shadow disabled.
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
    /// which logs and counts them and commits without the marker.
    pub(in crate::node) fn mirror_window(
        &self,
        entries: ShadowOverlayEntries,
    ) -> Result<Option<ShadowCommitMarker>, String> {
        let (block_index_min, block_index_max, state_root) = state_root_span(&entries);
        let mut state = self.state.lock();
        if state.pending.is_some() {
            return Err(
                "append shadow already has a frame awaiting canonical publication".to_owned(),
            );
        }
        let Some(writer) = state.writer.as_mut() else {
            return Err("append shadow writer is disabled after an earlier failure".to_owned());
        };
        let prepared = match writer.prepare_state_overlay(entries) {
            Ok(receipt) => receipt,
            Err(error) => {
                let message = format!("shadow pack prepare failed: {error:#}");
                state.writer = None;
                return Err(message);
            }
        };
        let Some(prepared) = prepared else {
            return Ok(None);
        };
        let receipt = prepared.receipt();
        state.pending = Some(prepared);
        let record =
            ShadowHighWaterRecord::new(&receipt, block_index_min, block_index_max, state_root);
        Ok(Some(ShadowCommitMarker {
            key: SHADOW_HIGH_WATER_KEY.to_vec(),
            value: record.encode(),
        }))
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
    let high_water = match load_high_water(canonical_store) {
        Ok(high_water) => high_water,
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

fn load_high_water(
    canonical_store: &RuntimeStore,
) -> anyhow::Result<Option<ShadowHighWaterRecord>> {
    let state_service = canonical_store
        .open_coordinated_namespace(neo_state_service::MDBX_STATE_SERVICE_NAMESPACE)
        .context("open coordinated StateService namespace for append shadow recovery")?;
    state_service
        .maintenance_metadata(SHADOW_HIGH_WATER_KEY)
        .context("read append shadow high-water marker")?
        .map(|bytes| {
            ShadowHighWaterRecord::decode(&bytes).context("decode append shadow high-water marker")
        })
        .transpose()
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
                if value.len() < STATE_ROOT_VALUE_UNSIGNED_LEN {
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
        let marker = shadow
            .mirror_window(entries)
            .expect("mirror first window")
            .expect("window with node entries yields a marker");
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
        let marker = shadow
            .mirror_window(entries)
            .expect("mirror second window")
            .expect("second window yields a marker");
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
        let marker = shadow
            .mirror_window(entries)
            .expect("metadata-only window must not fail");
        assert!(marker.is_none());
    }

    #[test]
    fn mirror_window_is_invisible_until_canonical_success() {
        let temp = tempfile::tempdir().expect("temp shadow root");
        let shadow_path = temp.path().join("shadow-packs");
        let writer = ShadowPackWriter::open_or_create(&shadow_path, test_pack_config())
            .expect("create shadow writer");
        let shadow = append_shadow(writer);
        let marker = shadow
            .mirror_window(vec![
                state_root_entry(41, 0x11),
                (node_key(1), Some(b"node-one".to_vec())),
            ])
            .expect("prepare shadow window")
            .expect("node window yields marker");

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
        let first = shadow
            .mirror_window(vec![
                state_root_entry(41, 0x11),
                (node_key(1), Some(b"committed".to_vec())),
            ])
            .expect("prepare first window")
            .expect("first marker");
        let committed = ShadowHighWaterRecord::decode(&first.value).expect("decode first marker");
        shadow.canonical_commit_succeeded();

        shadow
            .mirror_window(vec![
                state_root_entry(42, 0x22),
                (node_key(1), Some(b"orphan".to_vec())),
            ])
            .expect("prepare orphan window")
            .expect("orphan marker");
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
}
