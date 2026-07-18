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
    PackCommitHorizon, PackFrameReceipt,
};
use thiserror::Error;

/// MDBX maintenance-table key for authoritative node-pack publication.
pub const AUTHORITATIVE_HIGH_WATER_KEY: &[u8] = b"neo_state_packs_authoritative_high_water";

const MARKER_MAGIC: &[u8; 8] = b"N3PAWM01";
const MARKER_SCHEMA_VERSION: u32 = 1;

/// Exact encoded byte length of [`AuthoritativeHighWaterRecord`].
pub const AUTHORITATIVE_HIGH_WATER_RECORD_LEN: usize = 144;

/// Strict authoritative marker decode or identity error.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum AuthorityMarkerError {
    /// Record length differs from the fixed v1 layout.
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
        "authoritative marker pack formats ({frame}, {index}, {manifest}) differ from binary ({expected_frame}, {expected_index}, {expected_manifest})"
    )]
    PackFormatMismatch {
        /// Encoded frame format.
        frame: u32,
        /// Encoded index format.
        index: u32,
        /// Encoded manifest format.
        manifest: u32,
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
    /// Absolute end offset of that frame in `frames.pack`.
    pub frame_end: u64,
    /// SHA-256 checksum of the newest frame payload.
    pub frame_payload_sha256: [u8; 32],
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
            frame_end: receipt.frame_end,
            frame_payload_sha256: receipt.payload_sha256,
            block_index,
            state_root,
        }
    }

    /// Returns the exact pack prefix selected by this marker.
    pub const fn commit_horizon(&self) -> PackCommitHorizon {
        PackCommitHorizon {
            epoch: self.epoch,
            payload_sha256: self.frame_payload_sha256,
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

    /// Encodes the fixed, versioned v1 marker.
    pub fn encode(&self) -> [u8; AUTHORITATIVE_HIGH_WATER_RECORD_LEN] {
        let mut bytes = [0u8; AUTHORITATIVE_HIGH_WATER_RECORD_LEN];
        bytes[0..8].copy_from_slice(MARKER_MAGIC);
        bytes[8..12].copy_from_slice(&MARKER_SCHEMA_VERSION.to_le_bytes());
        bytes[12..16].copy_from_slice(&self.network_magic.to_le_bytes());
        bytes[16..20].copy_from_slice(&PACK_FRAME_FORMAT_VERSION.to_le_bytes());
        bytes[20..24].copy_from_slice(&PACK_INDEX_FORMAT_VERSION.to_le_bytes());
        bytes[24..28].copy_from_slice(&PACK_MANIFEST_FORMAT_VERSION.to_le_bytes());
        bytes[28..60].copy_from_slice(&self.store_identity);
        bytes[60..68].copy_from_slice(&self.epoch.to_le_bytes());
        bytes[68..76].copy_from_slice(&self.frame_end.to_le_bytes());
        bytes[76..108].copy_from_slice(&self.frame_payload_sha256);
        bytes[108..112].copy_from_slice(&self.block_index.to_le_bytes());
        bytes[112..144].copy_from_slice(&self.state_root);
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
        let frame = u32_at(bytes, 16);
        let index = u32_at(bytes, 20);
        let manifest = u32_at(bytes, 24);
        if (frame, index, manifest)
            != (
                PACK_FRAME_FORMAT_VERSION,
                PACK_INDEX_FORMAT_VERSION,
                PACK_MANIFEST_FORMAT_VERSION,
            )
        {
            return Err(AuthorityMarkerError::PackFormatMismatch {
                frame,
                index,
                manifest,
                expected_frame: PACK_FRAME_FORMAT_VERSION,
                expected_index: PACK_INDEX_FORMAT_VERSION,
                expected_manifest: PACK_MANIFEST_FORMAT_VERSION,
            });
        }
        let frame_end = u64_at(bytes, 68);
        if frame_end == 0 {
            return Err(AuthorityMarkerError::InvalidFrameEnd);
        }
        Ok(Self {
            network_magic: u32_at(bytes, 12),
            store_identity: bytes[28..60].try_into().expect("fixed identity range"),
            epoch: u64_at(bytes, 60),
            frame_end,
            frame_payload_sha256: bytes[76..108].try_into().expect("fixed checksum range"),
            block_index: u32_at(bytes, 108),
            state_root: bytes[112..144].try_into().expect("fixed root range"),
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
                frame_start: 1_000,
                frame_end: 2_000,
                rows: 3,
                payload_bytes: 928,
                payload_sha256: [0x22; 32],
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
                payload_sha256: [0x22; 32],
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
}
