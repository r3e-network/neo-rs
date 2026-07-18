//! # StateService shadow adapter
//!
//! Phase 1 of the append-frame rollout: MDBX remains the authoritative
//! commit path for every StateService row. The [`ShadowPackWriter`] mirrors
//! the MPT node entries (`0xf0 || node_hash`, 33-byte keys) of each
//! coordinated commit window into a [`PackStore`] in its own directory — one
//! frame plus one immutable index run per window. It is a verification
//! shadow: any error is returned to the caller, whose contract is to log and
//! ignore it (never fail the canonical commit).
//!
//! On success the caller also persists a [`ShadowHighWaterRecord`] into the
//! MDBX maintenance table inside the same canonical transaction (cold-first
//! ordering: the frame is synced before the marker commits). The marker is
//! the future commit authority for pack recovery; frames above the marker
//! are orphaned durable data.
//!
//! ## Boundary
//!
//! This adapter selects StateService MPT node rows and defines the versioned
//! marker payload. It does not open MDBX or decide node commit policy.
//!
//! ## Contents
//!
//! - High-water marker encoding and recovery horizon conversion.
//! - Durable prepare and post-marker activation for shadow frames.
//! - Bounded counters for frames and node operations.

use crate::{
    PACK_KEY_BYTES, PackCommitHorizon, PackFrameReceipt, PackOpKind, PackOperation, PackStore,
    PreparedAppend,
};
use anyhow::{Context, Result, ensure};
use std::path::Path;

/// Namespace prefix of StateService MPT node keys (`0xf0 || node_hash`).
/// Only keys with this prefix and exactly [`PACK_KEY_BYTES`] bytes are
/// mirrored into the shadow packs; StateService metadata records (`0x01`
/// state-root records, `0x02` current-root index) stay MDBX-only.
pub const STATE_NODE_KEY_PREFIX: u8 = 0xf0;

/// Maintenance-table key of the pack high-water marker. The record is
/// written inside the canonical MDBX transaction that also publishes the
/// mirrored overlay, so it can never point past a durable frame.
pub const SHADOW_HIGH_WATER_KEY: &[u8] = b"neo_state_packs_high_water";

const HIGH_WATER_MAGIC: &[u8; 8] = b"N3PHWM01";
const HIGH_WATER_FORMAT_VERSION: u32 = 1;
/// magic(8) + version(4) + epoch(8) + frames(8) + ops(8) + value bytes(8)
/// + payload checksum(32) + block min(4) + block max(4) + root(32).
pub const HIGH_WATER_RECORD_LEN: usize = 116;

const NO_BLOCK_INDEX: u32 = u32::MAX;

/// Outcome of one mirrored commit window.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowFrameReceipt {
    /// Commit epoch assigned to the mirrored frame.
    pub epoch: u64,
    /// Total durable frames in the shadow store after this append.
    pub frames_total: u64,
    /// Node operations mirrored in this frame (puts plus tombstones).
    pub node_operations: u64,
    /// Sum of put value bytes mirrored in this frame.
    pub node_put_value_bytes: u64,
    /// Placement and checksum of the underlying pack frame.
    pub frame: PackFrameReceipt,
}

/// One shadow frame made durable before its canonical MDBX marker commits.
///
/// The handle is intentionally opaque. It can only be activated by the same
/// writer after the caller confirms that the marker transaction committed.
#[derive(Debug, Clone, Copy)]
pub struct PreparedShadowFrame {
    pack: PreparedAppend,
    receipt: ShadowFrameReceipt,
}

impl PreparedShadowFrame {
    /// Marker data describing the durable, still-invisible frame.
    pub const fn receipt(self) -> ShadowFrameReceipt {
        self.receipt
    }
}

/// Commit-authority record persisted in the MDBX maintenance table under
/// [`SHADOW_HIGH_WATER_KEY`] for every successfully mirrored window.
///
/// Layout (all integers little-endian):
/// `magic(8) | version(4) | epoch(8) | frames_total(8) | node_operations(8)
/// | node_put_value_bytes(8) | frame payload SHA-256(32) | block_min(4)
/// | block_max(4) | state_root(32)`.
///
/// `block_min`/`block_max` are `u32::MAX` when the window carried no
/// state-root record; `state_root` is all-zero when unknown.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowHighWaterRecord {
    /// Commit epoch of the newest mirrored frame.
    pub epoch: u64,
    /// Total durable frames in the shadow store at marker publication.
    pub frames_total: u64,
    /// Node operations mirrored by the newest frame.
    pub node_operations: u64,
    /// Put value bytes mirrored by the newest frame.
    pub node_put_value_bytes: u64,
    /// SHA-256 checksum of the newest frame payload.
    pub frame_payload_sha256: [u8; 32],
    /// Lowest block index whose state-root record was mirrored, if any.
    pub block_index_min: Option<u32>,
    /// Highest block index whose state-root record was mirrored, if any.
    pub block_index_max: Option<u32>,
    /// State root hash of `block_index_max`, if observed.
    pub state_root: Option<[u8; 32]>,
}

impl ShadowHighWaterRecord {
    /// Builds the marker for one mirrored window.
    pub fn new(
        receipt: &ShadowFrameReceipt,
        block_index_min: Option<u32>,
        block_index_max: Option<u32>,
        state_root: Option<[u8; 32]>,
    ) -> Self {
        Self {
            epoch: receipt.epoch,
            frames_total: receipt.frames_total,
            node_operations: receipt.node_operations,
            node_put_value_bytes: receipt.node_put_value_bytes,
            frame_payload_sha256: receipt.frame.payload_sha256,
            block_index_min,
            block_index_max,
            state_root,
        }
    }

    /// Encodes the fixed-size marker record.
    pub fn encode(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(HIGH_WATER_RECORD_LEN);
        bytes.extend_from_slice(HIGH_WATER_MAGIC);
        bytes.extend_from_slice(&HIGH_WATER_FORMAT_VERSION.to_le_bytes());
        bytes.extend_from_slice(&self.epoch.to_le_bytes());
        bytes.extend_from_slice(&self.frames_total.to_le_bytes());
        bytes.extend_from_slice(&self.node_operations.to_le_bytes());
        bytes.extend_from_slice(&self.node_put_value_bytes.to_le_bytes());
        bytes.extend_from_slice(&self.frame_payload_sha256);
        bytes.extend_from_slice(&self.block_index_min.unwrap_or(NO_BLOCK_INDEX).to_le_bytes());
        bytes.extend_from_slice(&self.block_index_max.unwrap_or(NO_BLOCK_INDEX).to_le_bytes());
        bytes.extend_from_slice(&self.state_root.unwrap_or([0u8; 32]));
        debug_assert_eq!(bytes.len(), HIGH_WATER_RECORD_LEN);
        bytes
    }

    /// Decodes a marker record; returns `None` for any malformed input so
    /// recovery can treat an unreadable marker as "packs not committed".
    pub fn decode(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != HIGH_WATER_RECORD_LEN || &bytes[0..8] != HIGH_WATER_MAGIC {
            return None;
        }
        if u32::from_le_bytes(bytes[8..12].try_into().ok()?) != HIGH_WATER_FORMAT_VERSION {
            return None;
        }
        let u64_at = |offset: usize| -> Option<u64> {
            Some(u64::from_le_bytes(
                bytes.get(offset..offset + 8)?.try_into().ok()?,
            ))
        };
        let u32_at = |offset: usize| -> Option<u32> {
            Some(u32::from_le_bytes(
                bytes.get(offset..offset + 4)?.try_into().ok()?,
            ))
        };
        let epoch = u64_at(12)?;
        let frames_total = u64_at(20)?;
        if epoch.checked_add(1) != Some(frames_total) {
            return None;
        }
        let block_index_min = match u32_at(76)? {
            NO_BLOCK_INDEX => None,
            value => Some(value),
        };
        let block_index_max = match u32_at(80)? {
            NO_BLOCK_INDEX => None,
            value => Some(value),
        };
        let mut frame_payload_sha256 = [0u8; 32];
        frame_payload_sha256.copy_from_slice(bytes.get(44..76)?);
        let mut state_root = [0u8; 32];
        state_root.copy_from_slice(bytes.get(84..116)?);
        if block_index_min.is_some() != block_index_max.is_some()
            || block_index_min
                .zip(block_index_max)
                .is_some_and(|(min, max)| min > max)
        {
            return None;
        }
        let state_root = (state_root != [0u8; 32]).then_some(state_root);
        if state_root.is_some() && block_index_max.is_none() {
            return None;
        }
        Some(Self {
            epoch,
            frames_total,
            node_operations: u64_at(28)?,
            node_put_value_bytes: u64_at(36)?,
            frame_payload_sha256,
            block_index_min,
            block_index_max,
            state_root,
        })
    }

    /// Exact pack horizon authenticated by this canonical MDBX marker.
    pub const fn commit_horizon(&self) -> PackCommitHorizon {
        PackCommitHorizon {
            epoch: self.epoch,
            payload_sha256: self.frame_payload_sha256,
        }
    }
}

/// Mirrors StateService MPT overlay entries into a pack store in shadow
/// mode. The writer owns its own data directory and never touches the
/// canonical store. It is `Send` and safe to hold behind a mutex.
pub struct ShadowPackWriter {
    store: PackStore,
    frames_appended: u64,
    node_operations_appended: u64,
}

impl ShadowPackWriter {
    /// Opens the shadow store at `root`, creating it when missing. Reopen
    /// runs the engine's recovery (torn-tail truncation, derived-index
    /// rebuild), so an interrupted shadow append never wedges the writer.
    pub fn open_or_create(root: &Path, max_index_memory_bytes: u64) -> Result<Self> {
        let store = if root.join("frames.pack").exists() {
            PackStore::open(root, max_index_memory_bytes)
                .with_context(|| format!("reopen shadow pack store at {}", root.display()))?
        } else {
            PackStore::create(root, max_index_memory_bytes)
                .with_context(|| format!("create shadow pack store at {}", root.display()))?
        };
        Ok(Self {
            store,
            frames_appended: 0,
            node_operations_appended: 0,
        })
    }

    /// Opens a shadow store at the exact frame selected by the canonical
    /// MDBX high-water marker.
    ///
    /// `None` means no shadow frame committed. Complete pack frames above
    /// the marker are durable orphan suffixes and are discarded before the
    /// writer is returned. A marker without matching pack bytes is a
    /// corruption error; callers in shadow mode disable the writer while
    /// leaving MDBX authority untouched.
    pub fn open_or_create_at_high_water(
        root: &Path,
        max_index_memory_bytes: u64,
        high_water: Option<&ShadowHighWaterRecord>,
    ) -> Result<Self> {
        let store = if root.join("frames.pack").exists() {
            if let Some(high_water) = high_water {
                ensure!(
                    high_water.epoch.checked_add(1) == Some(high_water.frames_total),
                    "shadow high-water frame count is inconsistent"
                );
            }
            PackStore::open_at_commit_horizon(
                root,
                max_index_memory_bytes,
                high_water.map(|record| record.commit_horizon()),
            )
            .with_context(|| {
                format!(
                    "reconcile shadow pack store at {} to MDBX high-water marker",
                    root.display()
                )
            })?
        } else {
            ensure!(
                high_water.is_none(),
                "MDBX high-water marker exists but shadow pack store is missing at {}",
                root.display()
            );
            PackStore::create(root, max_index_memory_bytes)
                .with_context(|| format!("create shadow pack store at {}", root.display()))?
        };
        Ok(Self {
            store,
            frames_appended: 0,
            node_operations_appended: 0,
        })
    }

    /// Mirrors one coordinated commit window: every `0xf0 || node_hash`
    /// entry becomes a put (value) or tombstone (delete) in a single new
    /// frame; non-node StateService metadata keys are skipped.
    ///
    /// Returns `Ok(None)` when the window carries no node entries (no frame
    /// is appended and no marker should be published). Any engine error is
    /// returned to the caller, whose contract is to log, count, and continue
    /// the canonical commit without the marker.
    pub fn append_state_overlay(
        &mut self,
        entries: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    ) -> Result<Option<ShadowFrameReceipt>> {
        let Some(prepared) = self.prepare_state_overlay(entries)? else {
            return Ok(None);
        };
        self.activate_prepared(prepared).map(Some)
    }

    /// Durably writes one shadow frame without publishing it to the live view.
    ///
    /// The caller persists [`PreparedShadowFrame::receipt`] in its canonical
    /// transaction and invokes [`Self::activate_prepared`] only after that
    /// transaction commits.
    pub fn prepare_state_overlay(
        &mut self,
        entries: Vec<(Vec<u8>, Option<Vec<u8>>)>,
    ) -> Result<Option<PreparedShadowFrame>> {
        let mut operations = Vec::with_capacity(entries.len());
        let mut node_put_value_bytes = 0u64;
        for (key, value) in entries {
            if key.len() != PACK_KEY_BYTES || key.first() != Some(&STATE_NODE_KEY_PREFIX) {
                continue;
            }
            let mut operation_key = [0u8; PACK_KEY_BYTES];
            operation_key.copy_from_slice(&key);
            let kind = match value {
                Some(value) => {
                    node_put_value_bytes = node_put_value_bytes.saturating_add(value.len() as u64);
                    PackOpKind::Put(value)
                }
                None => PackOpKind::Tombstone,
            };
            operations.push(PackOperation {
                key: operation_key,
                kind,
            });
        }
        if operations.is_empty() {
            return Ok(None);
        }

        let pack = self.store.prepare_append(&operations)?;
        let frame = pack.receipt();
        Ok(Some(PreparedShadowFrame {
            pack,
            receipt: ShadowFrameReceipt {
                epoch: frame.epoch,
                frames_total: frame.epoch.saturating_add(1),
                node_operations: operations.len() as u64,
                node_put_value_bytes,
                frame,
            },
        }))
    }

    /// Publishes a prepared frame after its canonical marker committed.
    pub fn activate_prepared(
        &mut self,
        prepared: PreparedShadowFrame,
    ) -> Result<ShadowFrameReceipt> {
        self.store
            .activate_prepared(prepared.pack, prepared.pack.commit_horizon())?;
        self.frames_appended = self.frames_appended.saturating_add(1);
        self.node_operations_appended = self
            .node_operations_appended
            .saturating_add(prepared.receipt.node_operations);
        self.store
            .maintain()
            .context("maintain shadow indexes after marker activation")?;
        Ok(prepared.receipt)
    }

    /// Total frames mirrored by this writer handle.
    pub const fn frames_appended(&self) -> u64 {
        self.frames_appended
    }

    /// Total node operations mirrored by this writer handle.
    pub const fn node_operations_appended(&self) -> u64 {
        self.node_operations_appended
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    const INDEX_MEMORY_BOUND: u64 = 64 * 1024 * 1024;

    fn node_key(tag: u8) -> Vec<u8> {
        let mut key = vec![tag; PACK_KEY_BYTES];
        key[0] = STATE_NODE_KEY_PREFIX;
        key
    }

    fn state_root_record(index: u32) -> Vec<u8> {
        let mut key = vec![0u8; 5];
        key[0] = 0x01;
        key[1..].copy_from_slice(&index.to_be_bytes());
        key
    }

    fn window_one() -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
        vec![
            (state_root_record(41), Some(vec![9u8; 38])),
            (node_key(1), Some(b"node-one".to_vec())),
            (node_key(2), Some(b"node-two".to_vec())),
            (vec![0x02], Some(41u32.to_le_bytes().to_vec())),
        ]
    }

    fn window_two() -> Vec<(Vec<u8>, Option<Vec<u8>>)> {
        vec![
            (state_root_record(42), Some(vec![8u8; 38])),
            (node_key(1), Some(b"node-one-new".to_vec())),
            (node_key(2), None),
            (node_key(3), Some(b"node-three".to_vec())),
        ]
    }

    fn sorted_node_keys(tags: &[u8]) -> Vec<[u8; PACK_KEY_BYTES]> {
        let mut keys: Vec<[u8; PACK_KEY_BYTES]> = tags
            .iter()
            .map(|tag| node_key(*tag).try_into().expect("33-byte node key"))
            .collect();
        keys.sort();
        keys
    }

    #[test]
    fn mirrored_windows_round_trip_byte_for_byte_across_reopen() {
        let root = tempdir().expect("temporary shadow root");
        let mut writer = ShadowPackWriter::open_or_create(root.path(), INDEX_MEMORY_BOUND)
            .expect("create shadow writer");

        let first = writer
            .append_state_overlay(window_one())
            .expect("mirror first window")
            .expect("first window carries node entries");
        assert_eq!(first.epoch, 0);
        assert_eq!(first.frames_total, 1);
        assert_eq!(first.node_operations, 2);
        assert_eq!(
            first.node_put_value_bytes,
            (b"node-one".len() + b"node-two".len()) as u64
        );

        let second = writer
            .append_state_overlay(window_two())
            .expect("mirror second window")
            .expect("second window carries node entries");
        assert_eq!(second.epoch, 1);
        assert_eq!(second.node_operations, 3);
        assert_eq!(writer.frames_appended(), 2);
        assert_eq!(writer.node_operations_appended(), 5);
        drop(writer);

        let store = PackStore::open(root.path(), INDEX_MEMORY_BOUND).expect("reopen shadow packs");
        assert_eq!(store.open_validation().frames, 2);
        let keys = sorted_node_keys(&[1, 2, 3]);
        let results = store
            .get_many_sorted(&keys)
            .expect("batch read mirrored nodes");
        for (key, result) in keys.iter().zip(results.iter()) {
            let expected_value = match key[32] {
                1 => Some(b"node-one-new".to_vec()),
                2 => None,
                3 => Some(b"node-three".to_vec()),
                other => panic!("unexpected key tag {other}"),
            };
            assert_eq!(result, &expected_value, "node entry must mirror exactly");
        }
    }

    #[test]
    fn window_without_node_entries_appends_no_frame() {
        let root = tempdir().expect("temporary shadow root");
        let mut writer = ShadowPackWriter::open_or_create(root.path(), INDEX_MEMORY_BOUND)
            .expect("create shadow writer");
        let metadata_only = vec![
            (state_root_record(7), Some(vec![1u8; 38])),
            (vec![0x02], Some(7u32.to_le_bytes().to_vec())),
        ];
        let receipt = writer
            .append_state_overlay(metadata_only)
            .expect("metadata-only window must not fail");
        assert!(receipt.is_none());
        assert_eq!(writer.frames_appended(), 0);
        drop(writer);

        let store = PackStore::open(root.path(), INDEX_MEMORY_BOUND).expect("reopen shadow packs");
        assert_eq!(store.open_validation().frames, 0);
    }

    #[test]
    fn reopen_continues_epochs_contiguously() {
        let root = tempdir().expect("temporary shadow root");
        let mut writer = ShadowPackWriter::open_or_create(root.path(), INDEX_MEMORY_BOUND)
            .expect("create shadow writer");
        writer
            .append_state_overlay(window_one())
            .expect("mirror first window");
        drop(writer);

        let mut writer = ShadowPackWriter::open_or_create(root.path(), INDEX_MEMORY_BOUND)
            .expect("reopen shadow writer");
        let receipt = writer
            .append_state_overlay(window_two())
            .expect("mirror second window after reopen")
            .expect("second window carries node entries");
        assert_eq!(receipt.epoch, 1, "epochs must continue after reopen");
        assert_eq!(receipt.frames_total, 2);
    }

    #[test]
    fn high_water_reopen_discards_a_manifested_orphan_frame() {
        let root = tempdir().expect("temporary shadow root");
        let mut writer = ShadowPackWriter::open_or_create(root.path(), INDEX_MEMORY_BOUND)
            .expect("create shadow writer");
        let committed = writer
            .append_state_overlay(window_one())
            .expect("mirror committed window")
            .expect("committed window has nodes");
        let marker = ShadowHighWaterRecord::new(&committed, Some(41), Some(41), Some([7u8; 32]));
        writer
            .append_state_overlay(window_two())
            .expect("mirror orphan window")
            .expect("orphan window has nodes");
        drop(writer);

        let mut writer = ShadowPackWriter::open_or_create_at_high_water(
            root.path(),
            INDEX_MEMORY_BOUND,
            Some(&marker),
        )
        .expect("reconcile writer to MDBX marker");
        let replacement = writer
            .append_state_overlay(window_two())
            .expect("mirror replacement window")
            .expect("replacement window has nodes");
        assert_eq!(replacement.epoch, 1);
        assert_eq!(replacement.frames_total, 2);
        drop(writer);

        let store =
            PackStore::open(root.path(), INDEX_MEMORY_BOUND).expect("reopen reconciled pack");
        assert_eq!(store.open_validation().frames, 2);
    }

    #[test]
    fn high_water_record_round_trips() {
        let root = tempdir().expect("temporary shadow root");
        let mut writer = ShadowPackWriter::open_or_create(root.path(), INDEX_MEMORY_BOUND)
            .expect("create shadow writer");
        let receipt = writer
            .append_state_overlay(window_one())
            .expect("mirror window")
            .expect("window carries node entries");
        let record = ShadowHighWaterRecord::new(&receipt, Some(41), Some(41), Some([7u8; 32]));
        let decoded = ShadowHighWaterRecord::decode(&record.encode()).expect("decode marker");
        assert_eq!(decoded, record);
        assert_eq!(decoded.commit_horizon().epoch, receipt.epoch);
        assert_eq!(
            decoded.commit_horizon().payload_sha256,
            receipt.frame.payload_sha256
        );

        let sparse = ShadowHighWaterRecord::new(&receipt, None, None, None);
        let decoded =
            ShadowHighWaterRecord::decode(&sparse.encode()).expect("decode sparse marker");
        assert_eq!(decoded, sparse);
        assert_eq!(decoded.block_index_min, None);
        assert_eq!(decoded.state_root, None);

        assert!(ShadowHighWaterRecord::decode(&[]).is_none());
        assert!(ShadowHighWaterRecord::decode(&[0u8; HIGH_WATER_RECORD_LEN]).is_none());
        let mut truncated = record.encode();
        truncated.truncate(HIGH_WATER_RECORD_LEN - 1);
        assert!(ShadowHighWaterRecord::decode(&truncated).is_none());

        let mut inconsistent_frames = record.encode();
        inconsistent_frames[20..28].copy_from_slice(&99u64.to_le_bytes());
        assert!(ShadowHighWaterRecord::decode(&inconsistent_frames).is_none());
    }

    #[test]
    fn shadow_writer_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<ShadowPackWriter>();
    }
}
