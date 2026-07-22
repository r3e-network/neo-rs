//! # Authoritative pack checkpoint
//!
//! Strict schema-v4 metadata binding a complete materialized StateService node
//! namespace to one current pack-store horizon.
//!
//! ## Boundary
//!
//! This module owns checkpoint JSON decoding and format/identity validation.
//! It does not build packs, mutate the canonical MDBX marker, or decide when a
//! checkpoint becomes authoritative.
//!
//! ## Contents
//!
//! - [`PackCheckpoint`]: complete strict checkpoint JSON schema.
//! - [`ValidatedPackCheckpoint`]: decoded identity and tip binding.
//! - [`PackCheckpointError`]: typed read, decode, and validation failures.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use thiserror::Error;

use crate::{
    PACK_FRAME_FORMAT_VERSION, PACK_INDEX_FORMAT_VERSION, PACK_MANIFEST_FORMAT_VERSION,
    PACK_SEGMENT_FORMAT_VERSION, PACK_SEGMENT_HEADER_LEN, PackCommitHorizon, PackFrameContext,
    PackSegmentId,
};

/// Current strict checkpoint JSON schema.
pub const PACK_CHECKPOINT_SCHEMA_VERSION: u32 = 4;
/// File name published inside a complete pack checkpoint directory.
pub const PACK_CHECKPOINT_FILE: &str = "checkpoint.json";
/// Semantic source namespace materialized by an authoritative checkpoint.
pub const PACK_CHECKPOINT_SOURCE_NAMESPACE: &str = "neo_state_service";

const PACK_CHECKPOINT_MAX_BYTES: u64 = 64 * 1024;

/// Typed checkpoint metadata failures.
#[derive(Debug, Error)]
pub enum PackCheckpointError {
    /// The checkpoint file could not be read.
    #[error("read pack checkpoint {path}: {source}")]
    Read {
        /// Checkpoint path.
        path: PathBuf,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// Strict JSON decoding failed.
    #[error("decode pack checkpoint {path}: {source}")]
    Decode {
        /// Checkpoint path.
        path: PathBuf,
        /// Underlying JSON failure.
        #[source]
        source: serde_json::Error,
    },
    /// One stable checkpoint field violates the authoritative contract.
    #[error("invalid pack checkpoint {field}: {message}")]
    InvalidField {
        /// Field or related field group.
        field: &'static str,
        /// Stable diagnostic detail.
        message: String,
    },
}

/// Complete strict schema-v4 checkpoint report.
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct PackCheckpoint {
    /// JSON schema version.
    pub schema_version: u32,
    /// Whether all activation prerequisites completed.
    pub authoritative_ready: bool,
    /// Whether the source namespace was scanned without a row limit.
    pub complete: bool,
    /// Semantic source backend (`mdbx` for the legacy namespace).
    pub source_backend: String,
    /// Exact source namespace name.
    pub source_namespace: String,
    /// Uppercase `0x`-prefixed Neo network magic.
    pub network_magic: String,
    /// Frozen StateService source height.
    pub source_height: u32,
    /// Display-order StateService root.
    pub source_root: String,
    /// Internal little-endian StateService root bytes.
    pub source_root_internal_bytes: String,
    /// Domain-separated ordered live-namespace digest.
    pub source_namespace_sha256: String,
    /// Number of live source rows.
    pub rows: u64,
    /// Rows revalidated from an interrupted partial build.
    pub resumed_rows: u64,
    /// Sum of exact source value lengths.
    pub value_bytes: u64,
    /// Number of committed checkpoint frames.
    pub frames: u64,
    /// Configured target rows per frame.
    pub rows_per_frame: usize,
    /// Allocated append-pack bytes at completion.
    pub pack_bytes: u64,
    /// Bytes in live derived index runs.
    pub live_index_bytes: u64,
    /// Number of live derived index runs.
    pub live_runs: u64,
    /// Resident decoded index memory.
    pub decoded_index_memory_bytes: u64,
    /// Derived runs reclaimed before publication.
    pub gc_runs_deleted: u64,
    /// Superseded manifests reclaimed before publication.
    pub gc_manifests_deleted: u64,
    /// Physical bytes reclaimed before publication.
    pub gc_bytes_reclaimed: u64,
    /// Segment format version.
    pub pack_segment_format_version: u32,
    /// Frame format version.
    pub pack_frame_format_version: u32,
    /// Index format version.
    pub pack_index_format_version: u32,
    /// Manifest format version.
    pub pack_manifest_format_version: u32,
    /// Epoch of the last committed frame.
    pub tip_epoch: u64,
    /// Segment containing the last committed frame.
    pub tip_segment_id: u64,
    /// Exclusive byte end of the last committed frame.
    pub tip_frame_end: u64,
    /// SHA-256 receipt of the last committed frame.
    pub tip_frame_sha256: String,
    /// Frames validated by the full payload scrub.
    pub scrubbed_frames: u64,
    /// Rows validated by the full payload scrub.
    pub scrubbed_rows: u64,
    /// Puts validated by the full payload scrub.
    pub scrubbed_puts: u64,
    /// Tombstones validated by the full payload scrub.
    pub scrubbed_tombstones: u64,
    /// Metadata plus value bytes validated by the scrub.
    pub scrubbed_payload_bytes: u64,
    /// Exact value bytes validated by the scrub.
    pub scrubbed_value_bytes: u64,
    /// Wall seconds spent scrubbing.
    pub scrub_elapsed_seconds: f64,
    /// Complete builder wall seconds.
    pub elapsed_seconds: f64,
}

/// Decoded authoritative identity and current pack tip.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ValidatedPackCheckpoint {
    source_root_internal: [u8; 32],
    store_identity: [u8; 32],
    tip_frame_sha256: [u8; 32],
}

impl PackCheckpoint {
    /// Reads the strict checkpoint file from a pack root.
    pub fn read(pack_root: &Path) -> Result<Self, PackCheckpointError> {
        let path = pack_root.join(PACK_CHECKPOINT_FILE);
        let file = File::open(&path).map_err(|source| PackCheckpointError::Read {
            path: path.clone(),
            source,
        })?;
        let file_bytes = file
            .metadata()
            .map_err(|source| PackCheckpointError::Read {
                path: path.clone(),
                source,
            })?
            .len();
        ensure_field(
            file_bytes <= PACK_CHECKPOINT_MAX_BYTES,
            "file length",
            format!(
                "{file_bytes} bytes exceeds the {PACK_CHECKPOINT_MAX_BYTES}-byte checkpoint limit"
            ),
        )?;
        serde_json::from_reader(file.take(PACK_CHECKPOINT_MAX_BYTES + 1))
            .map_err(|source| PackCheckpointError::Decode { path, source })
    }

    /// Validates authority readiness, source identity, format tuple, complete
    /// scrub geometry, and all fixed-width hashes.
    pub fn validate_authoritative(
        &self,
        expected_network_magic: u32,
    ) -> Result<ValidatedPackCheckpoint, PackCheckpointError> {
        ensure_field(
            self.schema_version == PACK_CHECKPOINT_SCHEMA_VERSION,
            "schema_version",
            format!(
                "{} is unsupported; expected {PACK_CHECKPOINT_SCHEMA_VERSION}",
                self.schema_version
            ),
        )?;
        ensure_field(
            self.complete && self.authoritative_ready,
            "authority readiness",
            "checkpoint is not complete and authoritative-ready",
        )?;
        ensure_field(
            self.source_backend == "mdbx"
                && self.source_namespace == PACK_CHECKPOINT_SOURCE_NAMESPACE,
            "source",
            "expected the exact MDBX StateService namespace",
        )?;
        let network_magic = parse_network_magic(&self.network_magic)?;
        ensure_field(
            network_magic == expected_network_magic,
            "network_magic",
            format!("0x{network_magic:08X} differs from expected 0x{expected_network_magic:08X}"),
        )?;
        ensure_field(
            self.network_magic == format!("0x{network_magic:08X}"),
            "network_magic",
            "network magic is not in canonical uppercase 0x-prefixed form",
        )?;
        ensure_field(
            (
                self.pack_segment_format_version,
                self.pack_frame_format_version,
                self.pack_index_format_version,
                self.pack_manifest_format_version,
            ) == (
                PACK_SEGMENT_FORMAT_VERSION,
                PACK_FRAME_FORMAT_VERSION,
                PACK_INDEX_FORMAT_VERSION,
                PACK_MANIFEST_FORMAT_VERSION,
            ),
            "pack format tuple",
            "checkpoint formats differ from this binary",
        )?;
        ensure_field(
            self.rows > 0
                && self.frames > 0
                && self.rows_per_frame > 0
                && self.resumed_rows <= self.rows
                && self.live_runs > 0,
            "geometry",
            "checkpoint pack geometry is empty",
        )?;
        ensure_field(
            self.tip_frame_end > PACK_SEGMENT_HEADER_LEN,
            "tip_frame_end",
            "checkpoint pack tip does not extend past its segment header",
        )?;
        ensure_field(
            self.tip_segment_id <= self.tip_epoch,
            "tip_segment_id",
            "tip segment cannot be after the tip epoch",
        )?;
        ensure_field(
            self.tip_epoch.checked_add(1) == Some(self.frames),
            "tip_epoch",
            "tip epoch differs from frame count",
        )?;
        ensure_field(
            self.scrubbed_frames == self.frames
                && self.scrubbed_rows == self.rows
                && self.scrubbed_puts == self.rows
                && self.scrubbed_tombstones == 0
                && self.scrubbed_value_bytes == self.value_bytes,
            "scrub geometry",
            "full scrub differs from source geometry",
        )?;
        ensure_field(
            self.scrubbed_payload_bytes >= self.scrubbed_value_bytes,
            "scrubbed_payload_bytes",
            "payload bytes are smaller than value bytes",
        )?;
        let source_root_internal = decode_hash(
            &self.source_root_internal_bytes,
            "source_root_internal_bytes",
        )?;
        let mut displayed_root = source_root_internal;
        displayed_root.reverse();
        ensure_field(
            self.source_root == format!("0x{}", hex::encode(displayed_root)),
            "source_root",
            "display root does not match internal UInt256 bytes",
        )?;
        let expected_scrubbed_payload_bytes = self
            .rows
            .checked_mul(crate::PACK_FRAME_ROW_METADATA_BYTES as u64)
            .and_then(|bytes| bytes.checked_add(self.value_bytes))
            .ok_or_else(|| PackCheckpointError::InvalidField {
                field: "scrubbed_payload_bytes",
                message: "source geometry overflows u64".to_owned(),
            })?;
        ensure_field(
            self.scrubbed_payload_bytes == expected_scrubbed_payload_bytes,
            "scrubbed_payload_bytes",
            "payload bytes differ from exact frame metadata plus value geometry",
        )?;
        let store_identity = decode_hash(&self.source_namespace_sha256, "source_namespace_sha256")?;
        let tip_frame_sha256 = decode_hash(&self.tip_frame_sha256, "tip_frame_sha256")?;
        Ok(ValidatedPackCheckpoint {
            source_root_internal,
            store_identity,
            tip_frame_sha256,
        })
    }
}

impl ValidatedPackCheckpoint {
    /// Returns internal little-endian source root bytes.
    pub const fn source_root_internal(self) -> [u8; 32] {
        self.source_root_internal
    }

    /// Returns the domain-separated complete namespace identity.
    pub const fn store_identity(self) -> [u8; 32] {
        self.store_identity
    }

    /// Returns the authenticated tip-frame receipt digest.
    pub const fn tip_frame_sha256(self) -> [u8; 32] {
        self.tip_frame_sha256
    }

    /// Constructs the exact pack commit horizon bound by the checkpoint.
    pub fn commit_horizon(self, checkpoint: &PackCheckpoint) -> PackCommitHorizon {
        PackCommitHorizon {
            epoch: checkpoint.tip_epoch,
            segment_id: PackSegmentId::new(checkpoint.tip_segment_id),
            frame_end: checkpoint.tip_frame_end,
            context: PackFrameContext::new(
                checkpoint.source_height,
                checkpoint.source_height,
                self.source_root_internal,
                self.source_root_internal,
            ),
            frame_sha256: self.tip_frame_sha256,
        }
    }
}

fn parse_network_magic(value: &str) -> Result<u32, PackCheckpointError> {
    let encoded = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .unwrap_or(value);
    u32::from_str_radix(encoded, 16).map_err(|error| PackCheckpointError::InvalidField {
        field: "network_magic",
        message: error.to_string(),
    })
}

fn decode_hash(value: &str, field: &'static str) -> Result<[u8; 32], PackCheckpointError> {
    let encoded = value
        .strip_prefix("0x")
        .or_else(|| value.strip_prefix("0X"))
        .ok_or_else(|| PackCheckpointError::InvalidField {
            field,
            message: "expected a 0x-prefixed 32-byte hash".to_owned(),
        })?;
    let bytes = hex::decode(encoded).map_err(|error| PackCheckpointError::InvalidField {
        field,
        message: error.to_string(),
    })?;
    bytes
        .try_into()
        .map_err(|_| PackCheckpointError::InvalidField {
            field,
            message: "expected exactly 32 bytes".to_owned(),
        })
}

fn ensure_field(
    condition: bool,
    field: &'static str,
    message: impl Into<String>,
) -> Result<(), PackCheckpointError> {
    if condition {
        Ok(())
    } else {
        Err(PackCheckpointError::InvalidField {
            field,
            message: message.into(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    fn checkpoint() -> PackCheckpoint {
        PackCheckpoint {
            schema_version: PACK_CHECKPOINT_SCHEMA_VERSION,
            authoritative_ready: true,
            complete: true,
            source_backend: "mdbx".to_owned(),
            source_namespace: PACK_CHECKPOINT_SOURCE_NAMESPACE.to_owned(),
            network_magic: "0x334F454E".to_owned(),
            source_height: 123,
            source_root: format!("0x{}", hex::encode([0x55; 32])),
            source_root_internal_bytes: format!("0x{}", hex::encode([0x55; 32])),
            source_namespace_sha256: format!("0x{}", hex::encode([0x66; 32])),
            rows: 2,
            resumed_rows: 0,
            value_bytes: 3,
            frames: 1,
            rows_per_frame: 2,
            pack_bytes: 1024,
            live_index_bytes: 256,
            live_runs: 1,
            decoded_index_memory_bytes: 128,
            gc_runs_deleted: 0,
            gc_manifests_deleted: 0,
            gc_bytes_reclaimed: 0,
            pack_segment_format_version: PACK_SEGMENT_FORMAT_VERSION,
            pack_frame_format_version: PACK_FRAME_FORMAT_VERSION,
            pack_index_format_version: PACK_INDEX_FORMAT_VERSION,
            pack_manifest_format_version: PACK_MANIFEST_FORMAT_VERSION,
            tip_epoch: 0,
            tip_segment_id: 0,
            tip_frame_end: PACK_SEGMENT_HEADER_LEN + 512,
            tip_frame_sha256: format!("0x{}", hex::encode([0x77; 32])),
            scrubbed_frames: 1,
            scrubbed_rows: 2,
            scrubbed_puts: 2,
            scrubbed_tombstones: 0,
            scrubbed_payload_bytes: 115,
            scrubbed_value_bytes: 3,
            scrub_elapsed_seconds: 0.1,
            elapsed_seconds: 0.2,
        }
    }

    #[test]
    fn authoritative_validation_decodes_identity_and_horizon() {
        let checkpoint = checkpoint();
        let validated = checkpoint
            .validate_authoritative(0x334f_454e)
            .expect("validate checkpoint");
        assert_eq!(validated.store_identity(), [0x66; 32]);
        assert_eq!(validated.source_root_internal(), [0x55; 32]);
        let horizon = validated.commit_horizon(&checkpoint);
        assert_eq!(horizon.epoch, 0);
        assert_eq!(horizon.context.resulting_root, [0x55; 32]);
    }

    #[test]
    fn strict_schema_rejects_unknown_and_missing_fields() {
        let mut value = serde_json::to_value(checkpoint()).expect("encode checkpoint");
        value["unknown"] = serde_json::json!(1);
        assert!(serde_json::from_value::<PackCheckpoint>(value).is_err());

        let mut value = serde_json::to_value(checkpoint()).expect("encode checkpoint");
        value
            .as_object_mut()
            .expect("checkpoint object")
            .remove("tip_segment_id");
        assert!(serde_json::from_value::<PackCheckpoint>(value).is_err());
    }

    #[test]
    fn authoritative_validation_rejects_network_format_and_scrub_drift() {
        let mut candidate = checkpoint();
        assert!(candidate.validate_authoritative(0x3554_334e).is_err());
        candidate.pack_index_format_version += 1;
        assert!(candidate.validate_authoritative(0x334f_454e).is_err());
        candidate.pack_index_format_version = PACK_INDEX_FORMAT_VERSION;
        candidate.scrubbed_rows -= 1;
        assert!(candidate.validate_authoritative(0x334f_454e).is_err());
    }

    #[test]
    fn checkpoint_read_rejects_oversized_json_before_decode() {
        let temporary = tempdir().expect("temporary checkpoint root");
        std::fs::write(
            temporary.path().join(PACK_CHECKPOINT_FILE),
            vec![b' '; (PACK_CHECKPOINT_MAX_BYTES + 1) as usize],
        )
        .expect("write oversized checkpoint");
        assert!(matches!(
            PackCheckpoint::read(temporary.path()),
            Err(PackCheckpointError::InvalidField {
                field: "file length",
                ..
            })
        ));
    }
}
