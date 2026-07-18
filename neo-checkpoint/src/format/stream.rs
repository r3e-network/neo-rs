//! Canonical flat-row, Merkle-stream, and chunk-descriptor logic.

use super::codec::{
    Reader, digest, ensure_limit, ensure_version, put_bytes_u32, put_digest, put_u16, put_u64,
};
use crate::{
    BoundedBytes, CHECKPOINT_CHUNK_FORMAT_VERSION, CHECKPOINT_ROW_FIXED_BYTES,
    CHECKPOINT_ROW_FORMAT_VERSION, CheckpointError, CheckpointLimits, CheckpointResult,
    ChunkDescriptorV1, CompressionKindV1, Digest32, FlatRowV1, StreamCommitmentV1,
    StreamGeometryV1, StreamKind,
};
use sha2::{Digest, Sha256};

/// Fixed canonical flat-row magic.
pub const CHECKPOINT_ROW_MAGIC: [u8; 8] = *b"N3ROW001";
/// Fixed canonical chunk-descriptor magic.
pub const CHECKPOINT_CHUNK_DESCRIPTOR_MAGIC: [u8; 8] = *b"N3CHN001";

impl FlatRowV1 {
    /// Encodes one state or Ledger row canonically. The stream tag and ordinal
    /// are part of the Merkle leaf commitment.
    pub fn encode(
        &self,
        stream: StreamKind,
        limits: &CheckpointLimits,
    ) -> CheckpointResult<Vec<u8>> {
        validate_flat_row(self, stream, limits)?;
        let capacity = flat_row_encoded_len(self)?;
        ensure_limit("flat_row", capacity, limits.max_decoded_chunk_bytes)?;
        let mut output = Vec::with_capacity(capacity);
        output.extend_from_slice(&CHECKPOINT_ROW_MAGIC);
        put_u16(&mut output, self.version);
        output.push(stream as u8);
        put_u64(&mut output, self.ordinal);
        put_bytes_u32(&mut output, "row_key", &self.key)?;
        put_bytes_u32(&mut output, "row_value", &self.value)?;
        Ok(output)
    }

    /// Decodes exactly one complete canonical row and returns its stream kind.
    pub fn decode(bytes: &[u8], limits: &CheckpointLimits) -> CheckpointResult<(StreamKind, Self)> {
        ensure_limit("flat_row", bytes.len(), limits.max_decoded_chunk_bytes)?;
        let mut reader = Reader::new(bytes);
        if reader.take("flat_row_magic", CHECKPOINT_ROW_MAGIC.len())? != CHECKPOINT_ROW_MAGIC {
            return Err(CheckpointError::InvalidMagic { kind: "flat row" });
        }
        let version = reader.u16("flat_row_version")?;
        ensure_version("flat row", CHECKPOINT_ROW_FORMAT_VERSION, version)?;
        let stream = stream_kind("row_stream", reader.u8("row_stream")?)?;
        let ordinal = reader.u64("row_ordinal")?;
        let key = reader.bounded_bytes_u32("row_key", limits.max_key_bytes)?;
        let value = reader.bounded_bytes_u32("row_value", limits.max_value_bytes)?;
        reader.finish("flat row")?;
        let row = Self {
            version,
            ordinal,
            key,
            value,
        };
        validate_flat_row(&row, stream, limits)?;
        Ok((stream, row))
    }
}

impl ChunkDescriptorV1 {
    /// Constructs a V1 chunk descriptor. [`Self::validate`] remains mandatory
    /// before publication or use.
    #[allow(clippy::too_many_arguments)]
    pub const fn v1(
        stream: StreamKind,
        ordinal: u64,
        first_key: BoundedBytes,
        last_key: BoundedBytes,
        rows: u64,
        logical_bytes: u64,
        logical_hash: Digest32,
        encoded_bytes: u64,
        encoded_hash: Digest32,
        compression: CompressionKindV1,
    ) -> Self {
        Self {
            version: CHECKPOINT_CHUNK_FORMAT_VERSION,
            stream,
            ordinal,
            first_key,
            last_key,
            rows,
            logical_bytes,
            logical_hash,
            encoded_bytes,
            encoded_hash,
            compression,
        }
    }

    /// Validates canonical chunk geometry and local hard limits.
    pub fn validate(&self, limits: &CheckpointLimits) -> CheckpointResult<()> {
        ensure_version(
            "chunk descriptor",
            CHECKPOINT_CHUNK_FORMAT_VERSION,
            self.version,
        )?;
        if self.first_key.is_empty() || self.last_key.is_empty() {
            return Err(CheckpointError::InvalidField {
                field: "chunk_key_range",
                reason: "chunk key range has an empty endpoint",
            });
        }
        ensure_limit(
            "chunk_first_key",
            self.first_key.len(),
            limits.max_key_bytes,
        )?;
        ensure_limit("chunk_last_key", self.last_key.len(), limits.max_key_bytes)?;
        if self.first_key > self.last_key {
            return Err(CheckpointError::InvalidField {
                field: "chunk_key_range",
                reason: "chunk first key is greater than its last key",
            });
        }
        if self.rows == 0 {
            return Err(CheckpointError::InvalidField {
                field: "chunk_rows",
                reason: "chunk contains no rows",
            });
        }
        if self.rows > limits.max_rows {
            return Err(CheckpointError::LimitExceeded {
                field: "chunk_rows",
                actual: self.rows,
                maximum: limits.max_rows,
            });
        }
        if self.ordinal >= limits.max_chunks {
            return Err(CheckpointError::LimitExceeded {
                field: "chunk_ordinal",
                actual: self.ordinal,
                maximum: limits.max_chunks.saturating_sub(1),
            });
        }
        if self.logical_bytes == 0 || self.encoded_bytes == 0 {
            return Err(CheckpointError::InvalidField {
                field: "chunk_bytes",
                reason: "chunk has an empty logical or encoded payload",
            });
        }
        if self.logical_bytes > limits.max_decoded_chunk_bytes {
            return Err(CheckpointError::LimitExceeded {
                field: "chunk_logical_bytes",
                actual: self.logical_bytes,
                maximum: limits.max_decoded_chunk_bytes,
            });
        }
        if self.encoded_bytes > limits.max_encoded_chunk_bytes {
            return Err(CheckpointError::LimitExceeded {
                field: "chunk_encoded_bytes",
                actual: self.encoded_bytes,
                maximum: limits.max_encoded_chunk_bytes,
            });
        }
        if self.compression == CompressionKindV1::None
            && (self.logical_bytes != self.encoded_bytes || self.logical_hash != self.encoded_hash)
        {
            return Err(CheckpointError::InvalidField {
                field: "chunk_compression",
                reason: "uncompressed chunk encoded and logical identities differ",
            });
        }
        Ok(())
    }

    /// Validates a publishable V1 chunk. Uncompressed chunks remain available
    /// only for bounded diagnostic fixtures.
    pub fn validate_for_publication(&self, limits: &CheckpointLimits) -> CheckpointResult<()> {
        self.validate(limits)?;
        if self.compression != CompressionKindV1::Zstd {
            return Err(CheckpointError::InvalidField {
                field: "chunk_compression",
                reason: "published V1 chunks require the fixed zstd profile",
            });
        }
        Ok(())
    }

    /// Encodes the descriptor canonically.
    pub fn encode(&self, limits: &CheckpointLimits) -> CheckpointResult<Vec<u8>> {
        self.validate(limits)?;
        let capacity = chunk_descriptor_encoded_len(self)?;
        ensure_limit("chunk_descriptor", capacity, limits.max_core_bytes)?;
        let mut output = Vec::with_capacity(capacity);
        output.extend_from_slice(&CHECKPOINT_CHUNK_DESCRIPTOR_MAGIC);
        put_u16(&mut output, self.version);
        output.push(self.stream as u8);
        put_u64(&mut output, self.ordinal);
        put_bytes_u32(&mut output, "chunk_first_key", &self.first_key)?;
        put_bytes_u32(&mut output, "chunk_last_key", &self.last_key)?;
        put_u64(&mut output, self.rows);
        put_u64(&mut output, self.logical_bytes);
        put_digest(&mut output, self.logical_hash);
        put_u64(&mut output, self.encoded_bytes);
        put_digest(&mut output, self.encoded_hash);
        output.push(self.compression as u8);
        Ok(output)
    }

    /// Decodes exactly one canonical descriptor.
    pub fn decode(bytes: &[u8], limits: &CheckpointLimits) -> CheckpointResult<Self> {
        ensure_limit("chunk_descriptor", bytes.len(), limits.max_core_bytes)?;
        let mut reader = Reader::new(bytes);
        if reader.take(
            "chunk_descriptor_magic",
            CHECKPOINT_CHUNK_DESCRIPTOR_MAGIC.len(),
        )? != CHECKPOINT_CHUNK_DESCRIPTOR_MAGIC
        {
            return Err(CheckpointError::InvalidMagic {
                kind: "chunk descriptor",
            });
        }
        let version = reader.u16("chunk_descriptor_version")?;
        ensure_version("chunk descriptor", CHECKPOINT_CHUNK_FORMAT_VERSION, version)?;
        let stream = stream_kind("chunk_stream", reader.u8("chunk_stream")?)?;
        let ordinal = reader.u64("chunk_ordinal")?;
        let first_key = reader.bounded_bytes_u32("chunk_first_key", limits.max_key_bytes)?;
        let last_key = reader.bounded_bytes_u32("chunk_last_key", limits.max_key_bytes)?;
        let rows = reader.u64("chunk_rows")?;
        let logical_bytes = reader.u64("chunk_logical_bytes")?;
        let logical_hash = reader.digest("chunk_logical_hash")?;
        let encoded_bytes = reader.u64("chunk_encoded_bytes")?;
        let encoded_hash = reader.digest("chunk_encoded_hash")?;
        let compression = compression_kind(reader.u8("chunk_compression")?)?;
        reader.finish("chunk descriptor")?;
        let descriptor = Self {
            version,
            stream,
            ordinal,
            first_key,
            last_key,
            rows,
            logical_bytes,
            logical_hash,
            encoded_bytes,
            encoded_hash,
            compression,
        };
        descriptor.validate(limits)?;
        Ok(descriptor)
    }
}

/// Streaming RFC6962-style Merkle-tree accumulator.
#[derive(Debug, Clone, Default)]
pub struct MerkleAccumulatorV1 {
    leaf_count: u64,
    subtrees: Vec<Option<Digest32>>,
}

impl MerkleAccumulatorV1 {
    /// Constructs an empty accumulator.
    pub const fn new() -> Self {
        Self {
            leaf_count: 0,
            subtrees: Vec::new(),
        }
    }

    /// Adds one canonical record as a Merkle leaf.
    pub fn push_record(&mut self, record: &[u8]) -> CheckpointResult<()> {
        self.push_leaf_hash(merkle_leaf_hash(record))
    }

    /// Adds one already domain-separated leaf hash.
    pub fn push_leaf_hash(&mut self, mut current: Digest32) -> CheckpointResult<()> {
        let mut height = 0usize;
        while height < self.subtrees.len() {
            let Some(left) = self.subtrees[height].take() else {
                break;
            };
            current = merkle_node_hash(left, current);
            height = height
                .checked_add(1)
                .ok_or(CheckpointError::ArithmeticOverflow {
                    field: "merkle_height",
                })?;
        }
        if height == self.subtrees.len() {
            self.subtrees.push(Some(current));
        } else {
            self.subtrees[height] = Some(current);
        }
        self.leaf_count =
            self.leaf_count
                .checked_add(1)
                .ok_or(CheckpointError::ArithmeticOverflow {
                    field: "merkle_leaf_count",
                })?;
        Ok(())
    }

    /// Returns the accumulated leaf count.
    pub const fn leaf_count(&self) -> u64 {
        self.leaf_count
    }

    /// Returns the RFC6962-style root without consuming the accumulator.
    pub fn root(&self) -> Digest32 {
        let mut root = None;
        for subtree in self.subtrees.iter().flatten() {
            root = Some(match root {
                None => *subtree,
                Some(right) => merkle_node_hash(*subtree, right),
            });
        }
        root.unwrap_or_else(merkle_empty_root)
    }
}

/// Incremental exact geometry and commitment builder for state or Ledger rows.
#[derive(Debug, Clone)]
pub struct StreamCommitmentBuilderV1 {
    kind: StreamKind,
    last_key: Option<Vec<u8>>,
    key_bytes: u64,
    value_bytes: u64,
    logical_bytes: u64,
    merkle: MerkleAccumulatorV1,
}

impl StreamCommitmentBuilderV1 {
    /// Constructs a state or Ledger stream builder.
    pub fn try_new(kind: StreamKind) -> CheckpointResult<Self> {
        if !matches!(kind, StreamKind::State | StreamKind::Ledger) {
            return Err(CheckpointError::InvalidField {
                field: "stream_kind",
                reason: "flat-row builder supports only state or Ledger streams",
            });
        }
        Ok(Self {
            kind,
            last_key: None,
            key_bytes: 0,
            value_bytes: 0,
            logical_bytes: 0,
            merkle: MerkleAccumulatorV1::new(),
        })
    }

    /// Validates and commits the next exact row.
    pub fn push(&mut self, row: &FlatRowV1, limits: &CheckpointLimits) -> CheckpointResult<()> {
        if row.ordinal != self.merkle.leaf_count() {
            return Err(CheckpointError::InvalidField {
                field: "row_ordinal",
                reason: "row ordinal is not the next contiguous stream position",
            });
        }
        if self
            .last_key
            .as_deref()
            .is_some_and(|last| last >= row.key.as_slice())
        {
            return Err(CheckpointError::InvalidField {
                field: "row_key",
                reason: "row keys are not strictly increasing and unique",
            });
        }
        let encoded = row.encode(self.kind, limits)?;
        let rows =
            self.merkle
                .leaf_count()
                .checked_add(1)
                .ok_or(CheckpointError::ArithmeticOverflow {
                    field: "stream_rows",
                })?;
        if rows > limits.max_rows {
            return Err(CheckpointError::LimitExceeded {
                field: "stream_rows",
                actual: rows,
                maximum: limits.max_rows,
            });
        }
        let key_len =
            u64::try_from(row.key.len()).map_err(|_| CheckpointError::ArithmeticOverflow {
                field: "stream_key_bytes",
            })?;
        let value_len =
            u64::try_from(row.value.len()).map_err(|_| CheckpointError::ArithmeticOverflow {
                field: "stream_value_bytes",
            })?;
        let logical_len =
            u64::try_from(encoded.len()).map_err(|_| CheckpointError::ArithmeticOverflow {
                field: "stream_logical_bytes",
            })?;
        let key_bytes =
            self.key_bytes
                .checked_add(key_len)
                .ok_or(CheckpointError::ArithmeticOverflow {
                    field: "stream_key_bytes",
                })?;
        let value_bytes =
            self.value_bytes
                .checked_add(value_len)
                .ok_or(CheckpointError::ArithmeticOverflow {
                    field: "stream_value_bytes",
                })?;
        let logical_bytes = self.logical_bytes.checked_add(logical_len).ok_or(
            CheckpointError::ArithmeticOverflow {
                field: "stream_logical_bytes",
            },
        )?;
        if logical_bytes > limits.max_checkpoint_decoded_bytes {
            return Err(CheckpointError::LimitExceeded {
                field: "stream_logical_bytes",
                actual: logical_bytes,
                maximum: limits.max_checkpoint_decoded_bytes,
            });
        }
        self.merkle.push_record(&encoded)?;
        self.last_key = Some(row.key.as_slice().to_vec());
        self.key_bytes = key_bytes;
        self.value_bytes = value_bytes;
        self.logical_bytes = logical_bytes;
        Ok(())
    }

    /// Finalizes the stream with the exact externally produced chunk count.
    pub fn finish(
        self,
        chunks: u64,
        limits: &CheckpointLimits,
    ) -> CheckpointResult<StreamCommitmentV1> {
        let rows = self.merkle.leaf_count();
        if (rows == 0) != (chunks == 0) {
            return Err(CheckpointError::InvalidField {
                field: "stream_chunks",
                reason: "empty streams require zero chunks and nonempty streams require chunks",
            });
        }
        if chunks > limits.max_chunks {
            return Err(CheckpointError::LimitExceeded {
                field: "stream_chunks",
                actual: chunks,
                maximum: limits.max_chunks,
            });
        }
        Ok(StreamCommitmentV1 {
            kind: self.kind,
            root: self.merkle.root(),
            geometry: StreamGeometryV1 {
                rows,
                chunks,
                key_bytes: self.key_bytes,
                value_bytes: self.value_bytes,
                logical_bytes: self.logical_bytes,
            },
        })
    }
}

/// Returns the RFC6962 empty-tree hash.
pub fn merkle_empty_root() -> Digest32 {
    digest(Sha256::digest([]))
}

/// Hashes one canonical record as `SHA256(0x00 || record)`.
pub fn merkle_leaf_hash(record: &[u8]) -> Digest32 {
    let mut hasher = Sha256::new();
    hasher.update([0]);
    hasher.update(record);
    digest(hasher.finalize())
}

/// Hashes two ordered child roots as `SHA256(0x01 || left || right)`.
pub fn merkle_node_hash(left: Digest32, right: Digest32) -> Digest32 {
    let mut hasher = Sha256::new();
    hasher.update([1]);
    hasher.update(left.as_bytes());
    hasher.update(right.as_bytes());
    digest(hasher.finalize())
}

/// Computes the RFC6962-style root of canonical records with bounded streaming
/// memory.
pub fn merkle_root<'a>(records: impl IntoIterator<Item = &'a [u8]>) -> CheckpointResult<Digest32> {
    let mut accumulator = MerkleAccumulatorV1::new();
    for record in records {
        accumulator.push_record(record)?;
    }
    Ok(accumulator.root())
}

/// Validates a complete ordered descriptor set against committed stream
/// geometry. Logical row hashes remain authoritative and are recomputed while
/// decoding the chunks.
pub fn validate_chunk_descriptors(
    stream: StreamCommitmentV1,
    descriptors: &[ChunkDescriptorV1],
    limits: &CheckpointLimits,
) -> CheckpointResult<()> {
    let actual_chunks =
        u64::try_from(descriptors.len()).map_err(|_| CheckpointError::ArithmeticOverflow {
            field: "stream_chunks",
        })?;
    if actual_chunks != stream.geometry.chunks {
        return Err(CheckpointError::InvalidField {
            field: "stream_chunks",
            reason: "chunk descriptor count differs from committed geometry",
        });
    }
    let mut rows = 0u64;
    let mut logical_bytes = 0u64;
    let mut encoded_bytes = 0u64;
    let mut previous_last_key: Option<&[u8]> = None;
    for (index, descriptor) in descriptors.iter().enumerate() {
        descriptor.validate(limits)?;
        if descriptor.stream != stream.kind {
            return Err(CheckpointError::InvalidField {
                field: "chunk_stream",
                reason: "descriptor belongs to a different stream",
            });
        }
        let expected_ordinal =
            u64::try_from(index).map_err(|_| CheckpointError::ArithmeticOverflow {
                field: "chunk_ordinal",
            })?;
        if descriptor.ordinal != expected_ordinal {
            return Err(CheckpointError::InvalidField {
                field: "chunk_ordinal",
                reason: "chunk ordinals are not contiguous and ordered",
            });
        }
        if previous_last_key.is_some_and(|last| last >= descriptor.first_key.as_slice()) {
            return Err(CheckpointError::InvalidField {
                field: "chunk_key_range",
                reason: "chunk key ranges overlap or are out of order",
            });
        }
        rows = rows
            .checked_add(descriptor.rows)
            .ok_or(CheckpointError::ArithmeticOverflow {
                field: "chunk_rows",
            })?;
        logical_bytes = logical_bytes.checked_add(descriptor.logical_bytes).ok_or(
            CheckpointError::ArithmeticOverflow {
                field: "chunk_logical_bytes",
            },
        )?;
        encoded_bytes = encoded_bytes.checked_add(descriptor.encoded_bytes).ok_or(
            CheckpointError::ArithmeticOverflow {
                field: "chunk_encoded_bytes",
            },
        )?;
        previous_last_key = Some(descriptor.last_key.as_slice());
    }
    if rows != stream.geometry.rows || logical_bytes != stream.geometry.logical_bytes {
        return Err(CheckpointError::InvalidField {
            field: "chunk_geometry",
            reason: "chunk totals differ from committed stream geometry",
        });
    }
    if encoded_bytes > limits.max_checkpoint_encoded_bytes {
        return Err(CheckpointError::LimitExceeded {
            field: "checkpoint_encoded_bytes",
            actual: encoded_bytes,
            maximum: limits.max_checkpoint_encoded_bytes,
        });
    }
    Ok(())
}

fn validate_flat_row(
    row: &FlatRowV1,
    stream: StreamKind,
    limits: &CheckpointLimits,
) -> CheckpointResult<()> {
    ensure_version("flat row", CHECKPOINT_ROW_FORMAT_VERSION, row.version)?;
    if !matches!(stream, StreamKind::State | StreamKind::Ledger) {
        return Err(CheckpointError::InvalidField {
            field: "row_stream",
            reason: "flat rows support only state or Ledger streams",
        });
    }
    if row.key.is_empty() {
        return Err(CheckpointError::InvalidField {
            field: "row_key",
            reason: "raw StorageKey is empty",
        });
    }
    ensure_limit("row_key", row.key.len(), limits.max_key_bytes)?;
    ensure_limit("row_value", row.value.len(), limits.max_value_bytes)?;
    Ok(())
}

fn flat_row_encoded_len(row: &FlatRowV1) -> CheckpointResult<usize> {
    let fixed = usize::try_from(CHECKPOINT_ROW_FIXED_BYTES).expect("row framing fits usize");
    fixed
        .checked_add(row.key.len())
        .and_then(|value| value.checked_add(row.value.len()))
        .ok_or(CheckpointError::ArithmeticOverflow {
            field: "flat_row_bytes",
        })
}

fn chunk_descriptor_encoded_len(descriptor: &ChunkDescriptorV1) -> CheckpointResult<usize> {
    const FIXED: usize = 8 + 2 + 1 + 8 + 4 + 4 + 8 + 8 + 32 + 8 + 32 + 1;
    FIXED
        .checked_add(descriptor.first_key.len())
        .and_then(|value| value.checked_add(descriptor.last_key.len()))
        .ok_or(CheckpointError::ArithmeticOverflow {
            field: "chunk_descriptor_bytes",
        })
}

fn stream_kind(field: &'static str, value: u8) -> CheckpointResult<StreamKind> {
    match value {
        1 => Ok(StreamKind::State),
        2 => Ok(StreamKind::Ledger),
        3 => Ok(StreamKind::Headers),
        4 => Ok(StreamKind::Blocks),
        _ => Err(CheckpointError::InvalidTag { field, value }),
    }
}

fn compression_kind(value: u8) -> CheckpointResult<CompressionKindV1> {
    match value {
        0 => Ok(CompressionKindV1::None),
        1 => Ok(CompressionKindV1::Zstd),
        _ => Err(CheckpointError::InvalidTag {
            field: "chunk_compression",
            value,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(value: &[u8]) -> BoundedBytes {
        BoundedBytes::try_new("fixture", value.to_vec(), 1024).unwrap()
    }

    fn row(ordinal: u64, key: &[u8], value: &[u8]) -> FlatRowV1 {
        FlatRowV1::v1(ordinal, bytes(key), bytes(value))
    }

    fn reference_root(leaves: &[Digest32]) -> Digest32 {
        match leaves.len() {
            0 => merkle_empty_root(),
            1 => leaves[0],
            length => {
                let split = 1usize << ((length - 1).ilog2());
                merkle_node_hash(
                    reference_root(&leaves[..split]),
                    reference_root(&leaves[split..]),
                )
            }
        }
    }

    #[test]
    fn flat_row_round_trips_and_rejects_trailing_bytes() {
        let limits = CheckpointLimits::default();
        let row = row(7, &[1, 2], &[3, 4, 5]);
        let encoded = row.encode(StreamKind::State, &limits).unwrap();
        assert_eq!(
            FlatRowV1::decode(&encoded, &limits).unwrap(),
            (StreamKind::State, row)
        );
        let mut trailing = encoded;
        trailing.push(0);
        assert!(matches!(
            FlatRowV1::decode(&trailing, &limits),
            Err(CheckpointError::TrailingBytes { .. })
        ));
    }

    #[test]
    fn streaming_merkle_matches_recursive_rfc6962_shape() {
        let records: Vec<Vec<u8>> = (0u8..65).map(|value| vec![value]).collect();
        for length in 0..=records.len() {
            let leaves: Vec<_> = records[..length]
                .iter()
                .map(|record| merkle_leaf_hash(record))
                .collect();
            let mut accumulator = MerkleAccumulatorV1::new();
            for record in &records[..length] {
                accumulator.push_record(record).unwrap();
            }
            assert_eq!(accumulator.leaf_count(), length as u64);
            assert_eq!(accumulator.root(), reference_root(&leaves));
        }
    }

    #[test]
    fn stream_builder_enforces_ordinals_and_strict_key_order() {
        let limits = CheckpointLimits::default();
        let mut builder = StreamCommitmentBuilderV1::try_new(StreamKind::State).unwrap();
        builder.push(&row(0, &[1], &[10]), &limits).unwrap();
        builder.push(&row(1, &[2], &[20, 21]), &limits).unwrap();
        let commitment = builder.finish(1, &limits).unwrap();
        assert_eq!(commitment.geometry.rows, 2);
        assert_eq!(commitment.geometry.key_bytes, 2);
        assert_eq!(commitment.geometry.value_bytes, 3);
        assert_ne!(commitment.root, merkle_empty_root());

        let mut bad_ordinal = StreamCommitmentBuilderV1::try_new(StreamKind::Ledger).unwrap();
        assert!(bad_ordinal.push(&row(1, &[1], &[]), &limits).is_err());

        let mut duplicate = StreamCommitmentBuilderV1::try_new(StreamKind::Ledger).unwrap();
        duplicate.push(&row(0, &[1], &[]), &limits).unwrap();
        assert!(duplicate.push(&row(1, &[1], &[]), &limits).is_err());
    }

    #[test]
    fn chunk_descriptor_round_trips_and_enforces_publication_profile() {
        let limits = CheckpointLimits::default();
        let descriptor = ChunkDescriptorV1::v1(
            StreamKind::State,
            0,
            bytes(&[1]),
            bytes(&[2]),
            2,
            100,
            Digest32::from_bytes([3; 32]),
            80,
            Digest32::from_bytes([4; 32]),
            CompressionKindV1::Zstd,
        );
        descriptor.validate_for_publication(&limits).unwrap();
        let encoded = descriptor.encode(&limits).unwrap();
        assert_eq!(
            ChunkDescriptorV1::decode(&encoded, &limits).unwrap(),
            descriptor
        );

        let uncompressed = ChunkDescriptorV1::v1(
            StreamKind::Ledger,
            0,
            bytes(&[1]),
            bytes(&[1]),
            1,
            10,
            Digest32::from_bytes([5; 32]),
            10,
            Digest32::from_bytes([5; 32]),
            CompressionKindV1::None,
        );
        uncompressed.validate(&limits).unwrap();
        assert!(uncompressed.validate_for_publication(&limits).is_err());
    }

    #[test]
    fn chunk_descriptor_rejects_bad_range_and_size() {
        let mut limits = CheckpointLimits::default();
        limits.max_encoded_chunk_bytes = 10;
        let descriptor = ChunkDescriptorV1::v1(
            StreamKind::State,
            0,
            bytes(&[2]),
            bytes(&[1]),
            1,
            20,
            Digest32::default(),
            11,
            Digest32::default(),
            CompressionKindV1::Zstd,
        );
        assert!(descriptor.validate(&limits).is_err());
    }

    #[test]
    fn descriptor_set_rejects_gaps_overlaps_and_wrong_totals() {
        let limits = CheckpointLimits::default();
        let first = ChunkDescriptorV1::v1(
            StreamKind::State,
            0,
            bytes(&[1]),
            bytes(&[2]),
            1,
            30,
            Digest32::from_bytes([1; 32]),
            20,
            Digest32::from_bytes([2; 32]),
            CompressionKindV1::Zstd,
        );
        let second = ChunkDescriptorV1::v1(
            StreamKind::State,
            1,
            bytes(&[3]),
            bytes(&[4]),
            1,
            40,
            Digest32::from_bytes([3; 32]),
            25,
            Digest32::from_bytes([4; 32]),
            CompressionKindV1::Zstd,
        );
        let commitment = StreamCommitmentV1 {
            kind: StreamKind::State,
            root: Digest32::default(),
            geometry: StreamGeometryV1 {
                rows: 2,
                chunks: 2,
                key_bytes: 2,
                value_bytes: 14,
                logical_bytes: 70,
            },
        };
        validate_chunk_descriptors(commitment, &[first.clone(), second.clone()], &limits).unwrap();

        let mut bad_ordinal = second.clone();
        bad_ordinal.ordinal = 2;
        assert!(
            validate_chunk_descriptors(commitment, &[first.clone(), bad_ordinal], &limits).is_err()
        );

        let mut overlap = second.clone();
        overlap.first_key = bytes(&[2]);
        assert!(
            validate_chunk_descriptors(commitment, &[first.clone(), overlap], &limits).is_err()
        );

        let wrong_count = StreamCommitmentV1 {
            geometry: StreamGeometryV1 {
                rows: 3,
                ..commitment.geometry
            },
            ..commitment
        };
        assert!(validate_chunk_descriptors(wrong_count, &[first, second], &limits).is_err());
    }
}
