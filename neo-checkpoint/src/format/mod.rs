//! # Checkpoint format domain
//!
//! Canonical V1 checkpoint values, encodings, stream commitments, hostile-input
//! limits, and typed validation failures.
//!
//! ## Boundary
//!
//! This module owns deterministic logical checkpoint formats and pure bounded
//! validation. It accepts and returns in-memory values only; trust resolution,
//! storage, NeoFS transfer, import orchestration, and protocol execution remain
//! outside this crate.
//!
//! ## Contents
//!
//! - `bounds`: Format versions and local hostile-input limits.
//! - `codec`: Canonical core/certificate encoding and checkpoint identities.
//! - `error`: Typed format and validation failures.
//! - `stream`: Flat-row encoding, Merkle commitments, and chunk validation.
//! - `types`: Versioned checkpoint, transport, trust-anchor, and proof values.

mod bounds;
mod codec;
mod error;
mod stream;
mod types;

pub use bounds::{
    CHECKPOINT_CERTIFICATE_FORMAT_VERSION, CHECKPOINT_CHUNK_FORMAT_VERSION,
    CHECKPOINT_CORE_FORMAT_VERSION, CHECKPOINT_PROOF_FORMAT_VERSION, CHECKPOINT_ROW_FIXED_BYTES,
    CHECKPOINT_ROW_FORMAT_VERSION, CHECKPOINT_TRANSPORT_FORMAT_VERSION,
    CHECKPOINT_TRUST_ANCHOR_FORMAT_VERSION, CHECKPOINT_ZSTD_LEVEL, CheckpointLimits,
};
pub use codec::{
    CHECKPOINT_CERTIFICATE_SIGN_DOMAIN, CHECKPOINT_CORE_MAGIC, CHECKPOINT_ID_DOMAIN,
    checkpoint_certificate_sign_data, checkpoint_certificate_sign_hash,
};
pub use error::{CheckpointError, CheckpointResult};
pub use stream::{
    CHECKPOINT_CHUNK_DESCRIPTOR_MAGIC, CHECKPOINT_ROW_MAGIC, MerkleAccumulatorV1,
    StreamCommitmentBuilderV1, merkle_empty_root, merkle_leaf_hash, merkle_node_hash, merkle_root,
    validate_chunk_descriptors,
};
pub use types::{
    ArchiveCommitmentV1, BoundedBytes, CheckpointCertificateV1, CheckpointCoreV1, CheckpointId,
    ChunkDescriptorV1, ChunkLocationV1, CompressedPublicKey, CompressionKindV1, Digest32,
    FlatRowV1, LedgerBoundaryV1, LedgerPointProofV1, LedgerProofResultV1, MerkleInclusionProofV1,
    MptPointProofV1, ObjectLocatorV1, ProtocolIdentityV1, StreamCommitmentV1, StreamGeometryV1,
    StreamKind, TransactionInclusionProofV1, TransportManifestV1, TrustedCheckpointAnchorV1,
    ValidatorSetV1,
};
