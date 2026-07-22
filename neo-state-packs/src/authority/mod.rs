//! # Authoritative pack marker
//!
//! Unlike the optional shadow marker, this record is a mandatory part of the
//! canonical Ledger/StateService metadata transaction. Startup decodes it
//! strictly and opens the pack at exactly the named frame horizon.
//!
//! ## Boundary
//!
//! This module owns the stable marker byte format and identity validation. It
//! does not open MDBX, mutate packs, or decide canonical publication order.
//!
//! ## Contents
//!
//! - [`AuthoritativeHighWaterRecord`]: the canonical pack horizon binding.
//! - [`AuthorityMarkerError`]: strict decode and identity failures.
//! - [`AUTHORITATIVE_HIGH_WATER_KEY`]: the MDBX maintenance-table key.

use crate::{
    PACK_FRAME_FORMAT_VERSION, PACK_INDEX_FORMAT_VERSION, PACK_MANIFEST_FORMAT_VERSION,
    PACK_SEGMENT_FORMAT_VERSION, PACK_SEGMENT_HEADER_LEN, PackCommitHorizon, PackFrameContext,
    PackFrameReceipt, PackSegmentId,
};
use thiserror::Error;

/// MDBX maintenance-table key for authoritative node-pack publication.
pub const AUTHORITATIVE_HIGH_WATER_KEY: &[u8] = b"neo_state_packs_authoritative_high_water";

const MARKER_MAGIC: &[u8; 8] = b"N3PAWM01";
const MARKER_SCHEMA_VERSION: u32 = 3;

/// Exact encoded byte length of [`AuthoritativeHighWaterRecord`].
pub const AUTHORITATIVE_HIGH_WATER_RECORD_LEN: usize = 228;

/// Strict authoritative marker decode or identity error.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuthorityMarkerError {
    /// Record length differs from the fixed v3 layout.
    #[error("authoritative pack marker length is {actual}, expected {expected}")]
    InvalidLength {
        /// Observed byte length.
        actual: usize,
        /// Required byte length.
        expected: usize,
    },
    /// Magic does not identify this marker family.
    #[error("authoritative pack marker magic is invalid")]
    InvalidMagic,
    /// Marker schema is unsupported.
    #[error("authoritative pack marker schema {0} is unsupported")]
    UnsupportedSchema(u32),
    /// Pack format tuple differs from this binary.
    #[error(
        "authoritative marker pack formats ({segment}, {frame}, {index}, {manifest}) differ from binary ({expected_segment}, {expected_frame}, {expected_index}, {expected_manifest})"
    )]
    PackFormatMismatch {
        /// Encoded segment format.
        segment: u32,
        /// Encoded frame format.
        frame: u32,
        /// Encoded index format.
        index: u32,
        /// Encoded manifest format.
        manifest: u32,
        /// Binary segment format.
        expected_segment: u32,
        /// Binary frame format.
        expected_frame: u32,
        /// Binary index format.
        expected_index: u32,
        /// Binary manifest format.
        expected_manifest: u32,
    },
    /// Frame placement is structurally impossible.
    #[error("authoritative pack marker frame end is invalid")]
    InvalidFrameEnd,
    /// Segment identity is impossible for the named frame epoch.
    #[error("authoritative pack marker segment {segment_id} cannot contain frame epoch {epoch}")]
    InvalidSegmentId {
        /// Encoded segment identity.
        segment_id: PackSegmentId,
        /// Encoded frame epoch.
        epoch: u64,
    },
    /// The frame context names a reversed canonical block range.
    #[error("authoritative pack marker frame block range {block_start}..={block_end} is reversed")]
    InvalidBlockRange {
        /// First block represented by the frame.
        block_start: u32,
        /// Last block represented by the frame.
        block_end: u32,
    },
    /// The canonical StateService tip predates the newest durable frame.
    #[error(
        "authoritative StateService tip {block_index} predates frame ending at block {frame_block_end}"
    )]
    CanonicalTipBeforeFrame {
        /// Canonical StateService tip encoded by the marker.
        block_index: u32,
        /// Last block represented by the frame.
        frame_block_end: u32,
    },
    /// A canonical tip at the frame end does not equal the frame resulting root.
    #[error("authoritative marker canonical root differs from the frame resulting root")]
    CanonicalRootMismatch,
    /// Marker belongs to another Neo network.
    #[error("authoritative pack marker network {actual:#010x} differs from {expected:#010x}")]
    NetworkMismatch {
        /// Encoded network magic.
        actual: u32,
        /// Configured network magic.
        expected: u32,
    },
    /// Marker belongs to another base checkpoint/pack identity.
    #[error("authoritative pack marker store identity differs from the configured checkpoint")]
    StoreIdentityMismatch,
}

/// Canonical binding between StateService metadata and one durable pack tip.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AuthoritativeHighWaterRecord {
    /// Neo network magic.
    pub network_magic: u32,
    /// Stable SHA-256 identity of the complete base checkpoint.
    pub store_identity: [u8; 32],
    /// Newest canonical pack frame epoch.
    pub epoch: u64,
    /// Segment containing the canonical frame.
    pub segment_id: PackSegmentId,
    /// Segment-relative end offset of that frame.
    pub frame_end: u64,
    /// Domain-separated digest of the newest frame's authenticated header.
    ///
    /// The header binds both variable sections; the footer binds this digest
    /// to the epoch and exact complete-frame length.
    pub frame_sha256: [u8; 32],
    /// Exact canonical block/root transition represented by the newest frame.
    pub frame_context: PackFrameContext,
    /// Latest StateService local-root block index committed with this marker.
    pub block_index: u32,
    /// Internal UInt256 bytes of the corresponding StateService root.
    pub state_root: [u8; 32],
}

impl AuthoritativeHighWaterRecord {
    /// Constructs a marker from a sealed pack receipt and StateService tip.
    pub const fn new(
        network_magic: u32,
        store_identity: [u8; 32],
        receipt: PackFrameReceipt,
        block_index: u32,
        state_root: [u8; 32],
    ) -> Self {
        Self {
            network_magic,
            store_identity,
            epoch: receipt.epoch,
            segment_id: receipt.segment_id,
            frame_end: receipt.frame_end,
            frame_sha256: receipt.frame_sha256,
            frame_context: receipt.context,
            block_index,
            state_root,
        }
    }

    /// Returns the exact pack prefix selected by this marker.
    pub const fn commit_horizon(&self) -> PackCommitHorizon {
        PackCommitHorizon {
            epoch: self.epoch,
            segment_id: self.segment_id,
            frame_end: self.frame_end,
            context: self.frame_context,
            frame_sha256: self.frame_sha256,
        }
    }

    /// Rebinds the unchanged pack horizon to a metadata-only StateService tip.
    pub const fn with_state_tip(self, block_index: u32, state_root: [u8; 32]) -> Self {
        Self {
            block_index,
            state_root,
            ..self
        }
    }

    /// Encodes the fixed, versioned v3 marker.
    pub fn encode(&self) -> [u8; AUTHORITATIVE_HIGH_WATER_RECORD_LEN] {
        let mut bytes = [0u8; AUTHORITATIVE_HIGH_WATER_RECORD_LEN];
        bytes[0..8].copy_from_slice(MARKER_MAGIC);
        bytes[8..12].copy_from_slice(&MARKER_SCHEMA_VERSION.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.network_magic.to_le_bytes());
        bytes[16..20].copy_from_slice(&PACK_SEGMENT_FORMAT_VERSION.to_le_bytes());
        bytes[20..24].copy_from_slice(&PACK_FRAME_FORMAT_VERSION.to_le_bytes());
        bytes[24..28].copy_from_slice(&PACK_INDEX_FORMAT_VERSION.to_le_bytes());
        bytes[28..32].copy_from_slice(&PACK_MANIFEST_FORMAT_VERSION.to_le_bytes());
        bytes[32..64].copy_from_slice(&self.store_identity);
        bytes[64..72].copy_from_slice(&self.epoch.to_le_bytes());
        bytes[72..80].copy_from_slice(&self.segment_id.get().to_le_bytes());
        bytes[80..88].copy_from_slice(&self.frame_end.to_le_bytes());
        bytes[88..120].copy_from_slice(&self.frame_sha256);
        bytes[120..124].copy_from_slice(&self.frame_context.block_start.to_le_bytes());
        bytes[124..128].copy_from_slice(&self.frame_context.block_end.to_le_bytes());
        bytes[128..160].copy_from_slice(&self.frame_context.previous_root);
        bytes[160..192].copy_from_slice(&self.frame_context.resulting_root);
        bytes[192..196].copy_from_slice(&self.block_index.to_le_bytes());
        bytes[196..228].copy_from_slice(&self.state_root);
        bytes
    }

    /// Strictly decodes one marker and rejects format drift.
    pub fn decode(bytes: &[u8]) -> Result<Self, AuthorityMarkerError> {
        if bytes.len() != AUTHORITATIVE_HIGH_WATER_RECORD_LEN {
            return Err(AuthorityMarkerError::InvalidLength {
                actual: bytes.len(),
                expected: AUTHORITATIVE_HIGH_WATER_RECORD_LEN,
            });
        }
        if &bytes[0..8] != MARKER_MAGIC {
            return Err(AuthorityMarkerError::InvalidMagic);
        }
        let schema = u32_at(bytes, 8);
        if schema != MARKER_SCHEMA_VERSION {
            return Err(AuthorityMarkerError::UnsupportedSchema(schema));
        }
        let segment = u32_at(bytes, 16);
        let frame = u32_at(bytes, 20);
        let index = u32_at(bytes, 24);
        let manifest = u32_at(bytes, 28);
        if (segment, frame, index, manifest)
            != (
                PACK_SEGMENT_FORMAT_VERSION,
                PACK_FRAME_FORMAT_VERSION,
                PACK_INDEX_FORMAT_VERSION,
                PACK_MANIFEST_FORMAT_VERSION,
            )
        {
            return Err(AuthorityMarkerError::PackFormatMismatch {
                segment,
                frame,
                index,
                manifest,
                expected_segment: PACK_SEGMENT_FORMAT_VERSION,
                expected_frame: PACK_FRAME_FORMAT_VERSION,
                expected_index: PACK_INDEX_FORMAT_VERSION,
                expected_manifest: PACK_MANIFEST_FORMAT_VERSION,
            });
        }
        let epoch = u64_at(bytes, 64);
        let segment_id = PackSegmentId::new(u64_at(bytes, 72));
        if segment_id.get() > epoch {
            return Err(AuthorityMarkerError::InvalidSegmentId { segment_id, epoch });
        }
        let frame_end = u64_at(bytes, 80);
        if frame_end <= PACK_SEGMENT_HEADER_LEN {
            return Err(AuthorityMarkerError::InvalidFrameEnd);
        }
        let frame_context = PackFrameContext {
            block_start: u32_at(bytes, 120),
            block_end: u32_at(bytes, 124),
            previous_root: bytes[128..160]
                .try_into()
                .expect("fixed previous-root range"),
            resulting_root: bytes[160..192]
                .try_into()
                .expect("fixed resulting-root range"),
        };
        if frame_context.block_start > frame_context.block_end {
            return Err(AuthorityMarkerError::InvalidBlockRange {
                block_start: frame_context.block_start,
                block_end: frame_context.block_end,
            });
        }
        let block_index = u32_at(bytes, 192);
        if block_index < frame_context.block_end {
            return Err(AuthorityMarkerError::CanonicalTipBeforeFrame {
                block_index,
                frame_block_end: frame_context.block_end,
            });
        }
        let state_root: [u8; 32] = bytes[196..228]
            .try_into()
            .expect("fixed canonical root range");
        if block_index == frame_context.block_end && state_root != frame_context.resulting_root {
            return Err(AuthorityMarkerError::CanonicalRootMismatch);
        }
        Ok(Self {
            network_magic: u32_at(bytes, 12),
            store_identity: bytes[32..64].try_into().expect("fixed identity range"),
            epoch,
            segment_id,
            frame_end,
            frame_sha256: bytes[88..120].try_into().expect("fixed frame-digest range"),
            frame_context,
            block_index,
            state_root,
        })
    }

    /// Verifies the runtime network and persistent pack identity.
    pub fn validate_identity(
        &self,
        network_magic: u32,
        store_identity: [u8; 32],
    ) -> Result<(), AuthorityMarkerError> {
        if self.network_magic != network_magic {
            return Err(AuthorityMarkerError::NetworkMismatch {
                actual: self.network_magic,
                expected: network_magic,
            });
        }
        if self.store_identity != store_identity {
            return Err(AuthorityMarkerError::StoreIdentityMismatch);
        }
        Ok(())
    }
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("fixed u32 range"),
    )
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("fixed u64 range"),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn marker() -> AuthoritativeHighWaterRecord {
        AuthoritativeHighWaterRecord::new(
            0x334F_454E,
            [0x11; 32],
            PackFrameReceipt {
                epoch: 7,
                segment_id: PackSegmentId::new(3),
                frame_start: 1_000,
                frame_end: 2_000,
                context: PackFrameContext {
                    block_start: 40,
                    block_end: 42,
                    previous_root: [0x10; 32],
                    resulting_root: [0x33; 32],
                },
                rows: 3,
                metadata_bytes: 168,
                value_bytes: 760,
                frame_sha256: [0x22; 32],
            },
            42,
            [0x33; 32],
        )
    }

    #[test]
    fn authoritative_marker_round_trips_and_reuses_horizon_for_empty_windows() {
        let marker = marker();
        assert_eq!(
            AuthoritativeHighWaterRecord::decode(&marker.encode()).expect("decode marker"),
            marker
        );
        assert_eq!(
            marker.commit_horizon(),
            PackCommitHorizon {
                epoch: 7,
                segment_id: PackSegmentId::new(3),
                frame_end: 2_000,
                context: PackFrameContext {
                    block_start: 40,
                    block_end: 42,
                    previous_root: [0x10; 32],
                    resulting_root: [0x33; 32],
                },
                frame_sha256: [0x22; 32],
            }
        );
        let advanced = marker.with_state_tip(99, [0x44; 32]);
        assert_eq!(advanced.commit_horizon(), marker.commit_horizon());
        assert_eq!(advanced.block_index, 99);
        assert_eq!(advanced.state_root, [0x44; 32]);
    }

    #[test]
    fn authoritative_marker_rejects_corruption_and_identity_drift() {
        let marker = marker();
        let mut bytes = marker.encode();
        bytes[0] ^= 0xFF;
        assert_eq!(
            AuthoritativeHighWaterRecord::decode(&bytes),
            Err(AuthorityMarkerError::InvalidMagic)
        );
        assert!(matches!(
            marker.validate_identity(0xDEAD_BEEF, [0x11; 32]),
            Err(AuthorityMarkerError::NetworkMismatch { .. })
        ));
        assert_eq!(
            marker.validate_identity(0x334F_454E, [0x99; 32]),
            Err(AuthorityMarkerError::StoreIdentityMismatch)
        );
    }

    #[test]
    fn authoritative_marker_rejects_old_schema_and_segment_format_drift() {
        let marker = marker();

        let mut old_schema = marker.encode();
        old_schema[8..12].copy_from_slice(&(MARKER_SCHEMA_VERSION - 1).to_le_bytes());
        assert_eq!(
            AuthoritativeHighWaterRecord::decode(&old_schema),
            Err(AuthorityMarkerError::UnsupportedSchema(
                MARKER_SCHEMA_VERSION - 1
            ))
        );

        let mut wrong_segment_format = marker.encode();
        wrong_segment_format[16..20]
            .copy_from_slice(&(PACK_SEGMENT_FORMAT_VERSION + 1).to_le_bytes());
        assert!(matches!(
            AuthoritativeHighWaterRecord::decode(&wrong_segment_format),
            Err(AuthorityMarkerError::PackFormatMismatch {
                segment,
                expected_segment,
                ..
            }) if segment == PACK_SEGMENT_FORMAT_VERSION + 1
                && expected_segment == PACK_SEGMENT_FORMAT_VERSION
        ));

        let mut impossible_position = marker.encode();
        impossible_position[80..88].copy_from_slice(&PACK_SEGMENT_HEADER_LEN.to_le_bytes());
        assert_eq!(
            AuthoritativeHighWaterRecord::decode(&impossible_position),
            Err(AuthorityMarkerError::InvalidFrameEnd)
        );

        let mut impossible_segment = marker.encode();
        impossible_segment[72..80].copy_from_slice(&8u64.to_le_bytes());
        assert_eq!(
            AuthoritativeHighWaterRecord::decode(&impossible_segment),
            Err(AuthorityMarkerError::InvalidSegmentId {
                segment_id: PackSegmentId::new(8),
                epoch: 7,
            })
        );

        let mut reversed_range = marker.encode();
        reversed_range[120..124].copy_from_slice(&43u32.to_le_bytes());
        assert_eq!(
            AuthoritativeHighWaterRecord::decode(&reversed_range),
            Err(AuthorityMarkerError::InvalidBlockRange {
                block_start: 43,
                block_end: 42,
            })
        );

        let mut canonical_tip_before_frame = marker.encode();
        canonical_tip_before_frame[192..196].copy_from_slice(&41u32.to_le_bytes());
        assert_eq!(
            AuthoritativeHighWaterRecord::decode(&canonical_tip_before_frame),
            Err(AuthorityMarkerError::CanonicalTipBeforeFrame {
                block_index: 41,
                frame_block_end: 42,
            })
        );

        let mut canonical_root_mismatch = marker.encode();
        canonical_root_mismatch[196..228].fill(0x99);
        assert_eq!(
            AuthoritativeHighWaterRecord::decode(&canonical_root_mismatch),
            Err(AuthorityMarkerError::CanonicalRootMismatch)
        );
    }
}
