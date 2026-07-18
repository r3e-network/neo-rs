//! Versioned checkpoint, transport, trust-anchor, and proof value types.

use crate::{
    CHECKPOINT_CERTIFICATE_FORMAT_VERSION, CHECKPOINT_CORE_FORMAT_VERSION,
    CHECKPOINT_PROOF_FORMAT_VERSION, CHECKPOINT_ROW_FORMAT_VERSION,
    CHECKPOINT_TRANSPORT_FORMAT_VERSION, CHECKPOINT_TRUST_ANCHOR_FORMAT_VERSION, CheckpointError,
    CheckpointResult,
};

/// One SHA-256-sized digest in canonical byte order.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct Digest32([u8; 32]);

impl Digest32 {
    /// Constructs a digest from canonical bytes.
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Returns canonical digest bytes.
    pub const fn to_bytes(self) -> [u8; 32] {
        self.0
    }

    /// Borrows canonical digest bytes.
    pub const fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

/// Domain-separated identity of canonical [`CheckpointCoreV1`] bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct CheckpointId(Digest32);

impl CheckpointId {
    /// Constructs an identity from its digest.
    pub const fn new(digest: Digest32) -> Self {
        Self(digest)
    }

    /// Returns the underlying digest.
    pub const fn digest(self) -> Digest32 {
        self.0
    }
}

/// Immutable byte string whose construction enforces a caller-supplied hard
/// limit.
#[derive(Debug, Clone, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct BoundedBytes(Vec<u8>);

impl BoundedBytes {
    /// Constructs a bounded byte string.
    pub fn try_new(field: &'static str, bytes: Vec<u8>, maximum: u64) -> CheckpointResult<Self> {
        let actual = u64::try_from(bytes.len()).map_err(|_| CheckpointError::LimitExceeded {
            field,
            actual: u64::MAX,
            maximum,
        })?;
        if actual > maximum {
            return Err(CheckpointError::LimitExceeded {
                field,
                actual,
                maximum,
            });
        }
        Ok(Self(bytes))
    }

    /// Returns the byte slice.
    pub fn as_slice(&self) -> &[u8] {
        &self.0
    }

    /// Returns the byte length.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether no bytes are present.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Consumes the wrapper and returns its bytes.
    pub fn into_vec(self) -> Vec<u8> {
        self.0
    }
}

/// One compressed secp256r1 StateValidator public key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(transparent)]
pub struct CompressedPublicKey([u8; 33]);

impl CompressedPublicKey {
    /// Constructs a raw compressed key. Curve validation belongs to the trust
    /// verifier, not the format vocabulary.
    pub const fn from_bytes(bytes: [u8; 33]) -> Self {
        Self(bytes)
    }

    /// Returns compressed public-key bytes.
    pub const fn to_bytes(self) -> [u8; 33] {
        self.0
    }
}

/// Strictly ordered, unique, nonempty anchored validator set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidatorSetV1(Vec<CompressedPublicKey>);

impl ValidatorSetV1 {
    /// Constructs a validator set with structural bounds. Cryptographic key
    /// validation is performed by the trust verifier.
    pub fn try_new(validators: Vec<CompressedPublicKey>, maximum: u32) -> CheckpointResult<Self> {
        if validators.is_empty() {
            return Err(CheckpointError::InvalidField {
                field: "validators",
                reason: "validator set is empty",
            });
        }
        let actual =
            u64::try_from(validators.len()).map_err(|_| CheckpointError::LimitExceeded {
                field: "validators",
                actual: u64::MAX,
                maximum: u64::from(maximum),
            })?;
        if actual > u64::from(maximum) {
            return Err(CheckpointError::LimitExceeded {
                field: "validators",
                actual,
                maximum: u64::from(maximum),
            });
        }
        if validators.windows(2).any(|pair| pair[0] >= pair[1]) {
            return Err(CheckpointError::InvalidField {
                field: "validators",
                reason: "validator keys are not strictly ordered and unique",
            });
        }
        Ok(Self(validators))
    }

    /// Returns anchored validators in canonical order.
    pub fn as_slice(&self) -> &[CompressedPublicKey] {
        &self.0
    }

    /// Returns the number of validators.
    pub fn len(&self) -> usize {
        self.0.len()
    }

    /// Returns whether the set is empty. Valid constructed sets always return
    /// `false`.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

/// Logical checkpoint stream kind.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum StreamKind {
    /// Current live non-Ledger raw key/value rows.
    State = 1,
    /// Current live Ledger contract raw key/value rows.
    Ledger = 2,
    /// Canonical block-header segments.
    Headers = 3,
    /// Canonical full-block segments.
    Blocks = 4,
}

/// V1 chunk compression identity.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum CompressionKindV1 {
    /// Uncompressed diagnostic fixture.
    None = 0,
    /// Fixed V1 deterministic zstd profile.
    Zstd = 1,
}

/// Protocol implementation identity committed by a checkpoint.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProtocolIdentityV1 {
    /// Canonical ProtocolSettings digest.
    pub protocol_settings: Digest32,
    /// Hardfork schedule digest.
    pub hardforks: Digest32,
    /// Native-contract registry and implementation digest.
    pub native_contracts: Digest32,
    /// Canonical neo-vm implementation/image digest.
    pub neo_vm: Digest32,
    /// Canonical Neo MPT codec digest.
    pub mpt_codec: Digest32,
}

/// Exact geometry committed for one logical stream.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StreamGeometryV1 {
    /// Row count for state/Ledger streams, or record count for archives.
    pub rows: u64,
    /// Chunk count.
    pub chunks: u64,
    /// Aggregate raw key bytes.
    pub key_bytes: u64,
    /// Aggregate raw value bytes.
    pub value_bytes: u64,
    /// Aggregate canonical uncompressed bytes.
    pub logical_bytes: u64,
}

/// Merkle commitment and geometry for one logical stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct StreamCommitmentV1 {
    /// Stream kind.
    pub kind: StreamKind,
    /// Domain-separated Merkle root.
    pub root: Digest32,
    /// Committed exact geometry.
    pub geometry: StreamGeometryV1,
}

/// Commitment to contiguous header or block archive segments.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ArchiveCommitmentV1 {
    /// Header or block stream kind.
    pub kind: StreamKind,
    /// First represented height.
    pub first_height: u32,
    /// Last represented height.
    pub last_height: u32,
    /// Segment count.
    pub segments: u64,
    /// Logical record count.
    pub records: u64,
    /// Canonical logical bytes.
    pub logical_bytes: u64,
    /// Domain-separated segment-catalog root.
    pub root: Digest32,
}

/// Canonical V1 checkpoint statement. Transport locators and the detached
/// supplementary certificate are intentionally excluded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointCoreV1 {
    /// Core format version.
    pub version: u16,
    /// Neo network magic.
    pub network_magic: u32,
    /// Canonical genesis block hash bytes.
    pub genesis_hash: Digest32,
    /// Checkpoint height.
    pub height: u32,
    /// Canonical block hash at `height`.
    pub block_hash: Digest32,
    /// Locally expected protocol implementation identity.
    pub protocol: ProtocolIdentityV1,
    /// Complete signed N3 StateRoot wire bytes.
    pub signed_state_root: BoundedBytes,
    /// Hash of the independently anchored StateValidator set.
    pub state_validators_hash: Digest32,
    /// Current non-Ledger flat-state commitment.
    pub state: StreamCommitmentV1,
    /// Current Ledger flat-state commitment.
    pub ledger: StreamCommitmentV1,
    /// Header archive commitment through `height`.
    pub headers: ArchiveCommitmentV1,
    /// Full-block archive commitment through `height`.
    pub blocks: ArchiveCommitmentV1,
}

impl CheckpointCoreV1 {
    /// Returns a core with the only supported V1 format version.
    pub fn v1(
        network_magic: u32,
        genesis_hash: Digest32,
        height: u32,
        block_hash: Digest32,
        protocol: ProtocolIdentityV1,
        signed_state_root: BoundedBytes,
        state_validators_hash: Digest32,
        state: StreamCommitmentV1,
        ledger: StreamCommitmentV1,
        headers: ArchiveCommitmentV1,
        blocks: ArchiveCommitmentV1,
    ) -> Self {
        Self {
            version: CHECKPOINT_TRUST_ANCHOR_FORMAT_VERSION,
            network_magic,
            genesis_hash,
            height,
            block_hash,
            protocol,
            signed_state_root,
            state_validators_hash,
            state,
            ledger,
            headers,
            blocks,
        }
    }
}

/// Detached StateValidator quorum certificate over one checkpoint ID.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CheckpointCertificateV1 {
    /// Certificate format version.
    pub version: u16,
    /// Certified canonical checkpoint identity.
    pub checkpoint_id: CheckpointId,
    /// Canonical Neo multisignature witness bytes.
    pub witness: BoundedBytes,
}

impl CheckpointCertificateV1 {
    /// Returns a certificate envelope with the V1 format version.
    pub const fn v1(checkpoint_id: CheckpointId, witness: BoundedBytes) -> Self {
        Self {
            version: CHECKPOINT_CERTIFICATE_FORMAT_VERSION,
            checkpoint_id,
            witness,
        }
    }
}

/// Independently authenticated local trust anchor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustedCheckpointAnchorV1 {
    /// Trust-anchor format version.
    pub version: u16,
    /// Neo network magic.
    pub network_magic: u32,
    /// Canonical genesis block hash.
    pub genesis_hash: Digest32,
    /// Durable minimum automatically accepted checkpoint height.
    pub minimum_height: u32,
    /// Independently authenticated StateValidator keys.
    pub validators: ValidatorSetV1,
}

impl TrustedCheckpointAnchorV1 {
    /// Constructs a V1 anchor.
    pub const fn v1(
        network_magic: u32,
        genesis_hash: Digest32,
        minimum_height: u32,
        validators: ValidatorSetV1,
    ) -> Self {
        Self {
            version: CHECKPOINT_CORE_FORMAT_VERSION,
            network_magic,
            genesis_hash,
            minimum_height,
            validators,
        }
    }
}

/// One canonical raw state or Ledger row.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FlatRowV1 {
    /// Row format version.
    pub version: u16,
    /// Zero-based ordinal in the complete logical stream.
    pub ordinal: u64,
    /// Exact raw StorageKey bytes.
    pub key: BoundedBytes,
    /// Exact raw StorageItem bytes.
    pub value: BoundedBytes,
}

impl FlatRowV1 {
    /// Constructs a V1 row.
    pub const fn v1(ordinal: u64, key: BoundedBytes, value: BoundedBytes) -> Self {
        Self {
            version: CHECKPOINT_ROW_FORMAT_VERSION,
            ordinal,
            key,
            value,
        }
    }
}

/// One deterministic chunk descriptor committed by a stream manifest.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkDescriptorV1 {
    /// Chunk-descriptor format version.
    pub version: u16,
    /// Stream containing the chunk.
    pub stream: StreamKind,
    /// Zero-based stream-local ordinal.
    pub ordinal: u64,
    /// First raw key or archive record identity.
    pub first_key: BoundedBytes,
    /// Last raw key or archive record identity.
    pub last_key: BoundedBytes,
    /// Logical record count.
    pub rows: u64,
    /// Canonical decoded bytes.
    pub logical_bytes: u64,
    /// Canonical decoded SHA-256.
    pub logical_hash: Digest32,
    /// Encoded object bytes.
    pub encoded_bytes: u64,
    /// Encoded object SHA-256.
    pub encoded_hash: Digest32,
    /// Compression profile.
    pub compression: CompressionKindV1,
}

/// Replaceable object location for one authenticated chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ObjectLocatorV1 {
    /// NeoFS container and object IDs, encoded as bounded ASCII bytes.
    NeoFs {
        /// NeoFS container ID.
        container_id: BoundedBytes,
        /// NeoFS object ID.
        object_id: BoundedBytes,
    },
    /// HTTP(S) fallback URI, encoded as bounded ASCII bytes.
    Http {
        /// Absolute fallback URI.
        uri: BoundedBytes,
    },
    /// P2P content-addressed fallback.
    P2p {
        /// Requested encoded content digest.
        content_hash: Digest32,
    },
}

/// Locator set for one encoded chunk hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkLocationV1 {
    /// Encoded chunk hash named by the locators.
    pub encoded_hash: Digest32,
    /// Ordered alternate sources.
    pub locators: Vec<ObjectLocatorV1>,
}

/// Replaceable V1 mapping from logical checkpoint chunks to transport sources.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransportManifestV1 {
    /// Transport format version.
    pub version: u16,
    /// Logical checkpoint identity.
    pub checkpoint_id: CheckpointId,
    /// Chunk source mappings.
    pub chunks: Vec<ChunkLocationV1>,
}

impl TransportManifestV1 {
    /// Constructs a V1 transport manifest.
    pub const fn v1(checkpoint_id: CheckpointId, chunks: Vec<ChunkLocationV1>) -> Self {
        Self {
            version: CHECKPOINT_TRANSPORT_FORMAT_VERSION,
            checkpoint_id,
            chunks,
        }
    }
}

/// Merkle audit path whose direction is derived from `leaf_index` and
/// `leaf_count` by the canonical tree algorithm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MerkleInclusionProofV1 {
    /// Zero-based leaf index.
    pub leaf_index: u64,
    /// Total leaves in the committed tree.
    pub leaf_count: u64,
    /// Bottom-up sibling hashes.
    pub siblings: Vec<Digest32>,
}

/// Current non-Ledger state point proof against the N3 StateRoot.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MptPointProofV1 {
    /// Proof envelope format version.
    pub version: u16,
    /// Accepted checkpoint identity.
    pub checkpoint_id: CheckpointId,
    /// Signed StateRoot hash being opened.
    pub root: Digest32,
    /// Raw queried StorageKey bytes.
    pub key: BoundedBytes,
    /// Raw StorageItem bytes for membership, or `None` for non-membership.
    pub value: Option<BoundedBytes>,
    /// Canonical Neo MPT proof-node bytes.
    pub nodes: Vec<BoundedBytes>,
}

impl MptPointProofV1 {
    /// Returns a V1 proof envelope.
    pub const fn v1(
        checkpoint_id: CheckpointId,
        root: Digest32,
        key: BoundedBytes,
        value: Option<BoundedBytes>,
        nodes: Vec<BoundedBytes>,
    ) -> Self {
        Self {
            version: CHECKPOINT_PROOF_FORMAT_VERSION,
            checkpoint_id,
            root,
            key,
            value,
            nodes,
        }
    }
}

/// One proved Ledger row used for membership or an absence boundary.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerBoundaryV1 {
    /// Raw Ledger StorageKey bytes.
    pub key: BoundedBytes,
    /// Raw Ledger StorageItem bytes.
    pub value: BoundedBytes,
    /// Inclusion proof in the certified sorted Ledger tree.
    pub proof: MerkleInclusionProofV1,
}

/// Result committed by a Ledger point proof.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerProofResultV1 {
    /// Exact queried row and its inclusion proof.
    Present(LedgerBoundaryV1),
    /// Adjacent predecessor/successor or edge boundaries proving absence.
    Absent {
        /// Immediate predecessor, absent at the lower edge.
        lower: Option<LedgerBoundaryV1>,
        /// Immediate successor, absent at the upper edge.
        upper: Option<LedgerBoundaryV1>,
    },
}

/// Ledger row point proof against the checkpoint-certificate commitment.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerPointProofV1 {
    /// Proof envelope format version.
    pub version: u16,
    /// Accepted checkpoint identity.
    pub checkpoint_id: CheckpointId,
    /// Certified sorted Ledger root.
    pub root: Digest32,
    /// Raw queried key.
    pub key: BoundedBytes,
    /// Membership or adjacent-boundary absence result.
    pub result: LedgerProofResultV1,
}

/// Transaction inclusion proof against one verified block.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TransactionInclusionProofV1 {
    /// Proof envelope format version.
    pub version: u16,
    /// Accepted checkpoint identity.
    pub checkpoint_id: CheckpointId,
    /// Block height.
    pub height: u32,
    /// Canonical block hash.
    pub block_hash: Digest32,
    /// Transaction hash.
    pub transaction_hash: Digest32,
    /// Canonical transaction wire bytes.
    pub transaction: BoundedBytes,
    /// Zero-based transaction index in the block Merkle tree.
    pub transaction_index: u32,
    /// Number of transactions in the block.
    pub transaction_count: u32,
    /// Bottom-up transaction Merkle siblings.
    pub siblings: Vec<Digest32>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn key(byte: u8) -> CompressedPublicKey {
        CompressedPublicKey::from_bytes([byte; 33])
    }

    #[test]
    fn bounded_bytes_reject_input_above_limit() {
        assert_eq!(
            BoundedBytes::try_new("fixture", vec![1, 2, 3], 2),
            Err(CheckpointError::LimitExceeded {
                field: "fixture",
                actual: 3,
                maximum: 2,
            })
        );
        let bytes = BoundedBytes::try_new("fixture", vec![1, 2], 2).unwrap();
        assert_eq!(bytes.as_slice(), &[1, 2]);
    }

    #[test]
    fn validator_set_is_nonempty_bounded_strictly_ordered_and_unique() {
        let validators = ValidatorSetV1::try_new(vec![key(1), key(2)], 2).unwrap();
        assert_eq!(validators.len(), 2);
        assert!(!validators.is_empty());

        assert!(ValidatorSetV1::try_new(Vec::new(), 2).is_err());
        assert!(ValidatorSetV1::try_new(vec![key(1), key(1)], 2).is_err());
        assert!(ValidatorSetV1::try_new(vec![key(2), key(1)], 2).is_err());
        assert_eq!(
            ValidatorSetV1::try_new(vec![key(1), key(2), key(3)], 2),
            Err(CheckpointError::LimitExceeded {
                field: "validators",
                actual: 3,
                maximum: 2,
            })
        );
    }

    #[test]
    fn v1_constructors_pin_explicit_versions() {
        let validators = ValidatorSetV1::try_new(vec![key(1)], 1).unwrap();
        let anchor = TrustedCheckpointAnchorV1::v1(1, Digest32::default(), 2, validators);
        assert_eq!(anchor.version, CHECKPOINT_CORE_FORMAT_VERSION);

        let id = CheckpointId::new(Digest32::from_bytes([3; 32]));
        let certificate =
            CheckpointCertificateV1::v1(id, BoundedBytes::try_new("witness", vec![4], 1).unwrap());
        assert_eq!(certificate.version, CHECKPOINT_CERTIFICATE_FORMAT_VERSION);
        assert_eq!(certificate.checkpoint_id, id);

        let transport = TransportManifestV1::v1(id, Vec::new());
        assert_eq!(transport.version, CHECKPOINT_TRANSPORT_FORMAT_VERSION);
    }

    #[test]
    fn locator_duplicates_are_detectable_without_trusting_order() {
        let hash = Digest32::from_bytes([7; 32]);
        let locations = [
            ChunkLocationV1 {
                encoded_hash: hash,
                locators: Vec::new(),
            },
            ChunkLocationV1 {
                encoded_hash: hash,
                locators: Vec::new(),
            },
        ];
        let unique: HashSet<_> = locations
            .iter()
            .map(|location| location.encoded_hash)
            .collect();
        assert_ne!(unique.len(), locations.len());
    }
}
