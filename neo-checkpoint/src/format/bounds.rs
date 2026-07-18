//! Format versions and default hostile-input limits.

/// Canonical checkpoint-core format version.
pub const CHECKPOINT_CORE_FORMAT_VERSION: u16 = 1;
/// Supplementary checkpoint-certificate format version.
pub const CHECKPOINT_CERTIFICATE_FORMAT_VERSION: u16 = 1;
/// Transport-manifest format version.
pub const CHECKPOINT_TRANSPORT_FORMAT_VERSION: u16 = 1;
/// Light point-proof envelope format version.
pub const CHECKPOINT_PROOF_FORMAT_VERSION: u16 = 1;
/// Flat state/Ledger row format version.
pub const CHECKPOINT_ROW_FORMAT_VERSION: u16 = 1;
/// Chunk-descriptor format version.
pub const CHECKPOINT_CHUNK_FORMAT_VERSION: u16 = 1;
/// Local trust-anchor format version.
pub const CHECKPOINT_TRUST_ANCHOR_FORMAT_VERSION: u16 = 1;

/// Fixed zstd level identified by [`CompressionKindV1::Zstd`](crate::CompressionKindV1::Zstd).
pub const CHECKPOINT_ZSTD_LEVEL: i32 = 3;
/// Fixed bytes before one row's key and value payloads.
pub const CHECKPOINT_ROW_FIXED_BYTES: u64 = 8 + 2 + 1 + 8 + 4 + 4;

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;
const TIB: u64 = 1024 * GIB;

/// Local hard limits applied before allocating or decoding untrusted
/// checkpoint input.
///
/// These limits are policy, not checkpoint identity. A checkpoint may declare
/// lower geometry, but downloaded data cannot raise a verifier's local limits.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CheckpointLimits {
    /// Maximum canonical checkpoint-core bytes.
    pub max_core_bytes: u64,
    /// Maximum detached certificate bytes.
    pub max_certificate_bytes: u64,
    /// Maximum transport-manifest bytes.
    pub max_transport_manifest_bytes: u64,
    /// Maximum number of chunks across all products.
    pub max_chunks: u64,
    /// Maximum number of state and Ledger rows.
    pub max_rows: u64,
    /// Maximum raw key length.
    pub max_key_bytes: u64,
    /// Maximum raw value length.
    pub max_value_bytes: u64,
    /// Maximum encoded bytes in one NeoFS object.
    pub max_encoded_chunk_bytes: u64,
    /// Maximum decoded logical bytes in one chunk.
    pub max_decoded_chunk_bytes: u64,
    /// Maximum aggregate encoded checkpoint bytes.
    pub max_checkpoint_encoded_bytes: u64,
    /// Maximum aggregate decoded checkpoint bytes.
    pub max_checkpoint_decoded_bytes: u64,
    /// Maximum anchored StateValidators.
    pub max_validators: u32,
    /// Maximum alternate transport locators per chunk.
    pub max_locators_per_chunk: u32,
    /// Maximum encoded bytes in one locator.
    pub max_locator_bytes: u64,
    /// Maximum nodes or siblings in one point proof.
    pub max_proof_nodes: u32,
    /// Maximum total bytes in one point proof.
    pub max_proof_bytes: u64,
}

impl Default for CheckpointLimits {
    fn default() -> Self {
        Self {
            max_core_bytes: MIB,
            max_certificate_bytes: MIB,
            max_transport_manifest_bytes: 16 * MIB,
            max_chunks: 1_000_000,
            max_rows: 1_000_000_000,
            max_key_bytes: 4 * 1024,
            max_value_bytes: 16 * MIB,
            max_encoded_chunk_bytes: 48 * MIB,
            max_decoded_chunk_bytes: 64 * MIB,
            max_checkpoint_encoded_bytes: TIB,
            max_checkpoint_decoded_bytes: 2 * TIB,
            max_validators: 1_024,
            max_locators_per_chunk: 16,
            max_locator_bytes: 16 * 1024,
            max_proof_nodes: 4_096,
            max_proof_bytes: 16 * MIB,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_limits_are_nonzero_and_keep_objects_below_neofs_maximum() {
        let limits = CheckpointLimits::default();
        assert_eq!(limits.max_encoded_chunk_bytes, 48 * MIB);
        assert!(limits.max_encoded_chunk_bytes < 64 * MIB);
        assert!(limits.max_decoded_chunk_bytes >= limits.max_encoded_chunk_bytes);
        assert!(limits.max_checkpoint_decoded_bytes >= limits.max_checkpoint_encoded_bytes);
        assert!(limits.max_chunks > 0);
        assert!(limits.max_rows > 0);
        assert!(limits.max_validators > 0);
    }
}
