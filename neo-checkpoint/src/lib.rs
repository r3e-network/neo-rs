//! # Neo checkpoint formats
//!
//! This crate owns bounded V1 data types and format identities. It deliberately
//! has no storage, network, NeoFS, VM, StateService, or node-composition
//! dependency. Canonical codecs, cryptographic verification, transport, export,
//! and import are layered onto these types by later implementation tasks.
//!
//! NeoFS object identifiers are transport locations only. The authoritative
//! logical identity is [`CheckpointId`], derived from canonical
//! [`CheckpointCoreV1`] bytes.
//!
//! ## Boundary
//!
//! This crate owns dependency-light checkpoint format values, canonical
//! encodings, bounded geometry validation, and deterministic commitments. It
//! does not own checkpoint trust verification, persistence, import, transport,
//! node startup, StateService integration, or NeoVM execution.
//!
//! ## Contents
//!
//! - `format`: Versioned values, limits, codecs, stream commitments, and typed
//!   format errors.

mod format;

pub use format::{
    ArchiveCommitmentV1, BoundedBytes, CheckpointCertificateV1, CheckpointCoreV1, CheckpointId,
    ChunkDescriptorV1, ChunkLocationV1, CompressedPublicKey, CompressionKindV1, Digest32,
    FlatRowV1, LedgerBoundaryV1, LedgerPointProofV1, LedgerProofResultV1, MerkleInclusionProofV1,
    MptPointProofV1, ObjectLocatorV1, ProtocolIdentityV1, StreamCommitmentV1, StreamGeometryV1,
    StreamKind, TransactionInclusionProofV1, TransportManifestV1, TrustedCheckpointAnchorV1,
    ValidatorSetV1,
};
pub use format::{
    CHECKPOINT_CERTIFICATE_FORMAT_VERSION, CHECKPOINT_CHUNK_FORMAT_VERSION,
    CHECKPOINT_CORE_FORMAT_VERSION, CHECKPOINT_PROOF_FORMAT_VERSION, CHECKPOINT_ROW_FIXED_BYTES,
    CHECKPOINT_ROW_FORMAT_VERSION, CHECKPOINT_TRANSPORT_FORMAT_VERSION,
    CHECKPOINT_TRUST_ANCHOR_FORMAT_VERSION, CHECKPOINT_ZSTD_LEVEL, CheckpointLimits,
};
pub use format::{
    CHECKPOINT_CERTIFICATE_SIGN_DOMAIN, CHECKPOINT_CORE_MAGIC, CHECKPOINT_ID_DOMAIN,
    checkpoint_certificate_sign_data, checkpoint_certificate_sign_hash,
};
pub use format::{
    CHECKPOINT_CHUNK_DESCRIPTOR_MAGIC, CHECKPOINT_ROW_MAGIC, MerkleAccumulatorV1,
    StreamCommitmentBuilderV1, merkle_empty_root, merkle_leaf_hash, merkle_node_hash, merkle_root,
    validate_chunk_descriptors,
};
pub use format::{CheckpointError, CheckpointResult};
