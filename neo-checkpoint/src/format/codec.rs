//! Canonical V1 checkpoint-core and certificate codecs.

use crate::{
    ArchiveCommitmentV1, BoundedBytes, CHECKPOINT_CERTIFICATE_FORMAT_VERSION,
    CHECKPOINT_CORE_FORMAT_VERSION, CHECKPOINT_ROW_FIXED_BYTES, CheckpointCertificateV1,
    CheckpointCoreV1, CheckpointError, CheckpointId, CheckpointLimits, CheckpointResult, Digest32,
    ProtocolIdentityV1, StreamCommitmentV1, StreamGeometryV1, StreamKind,
};
use sha2::{Digest, Sha256};

/// Fixed canonical checkpoint-core magic.
pub const CHECKPOINT_CORE_MAGIC: [u8; 8] = *b"N3CKPT01";
const CHECKPOINT_CERTIFICATE_MAGIC: [u8; 8] = *b"N3CERT01";
/// Domain used to derive a logical checkpoint identity from core bytes.
pub const CHECKPOINT_ID_DOMAIN: &[u8] = b"neo-checkpoint-v1\0";
/// Domain used to derive supplementary certificate sign data.
pub const CHECKPOINT_CERTIFICATE_SIGN_DOMAIN: &[u8] = b"neo-checkpoint-certificate-v1\0";

impl CheckpointCoreV1 {
    /// Encodes the core in its canonical V1 binary form.
    pub fn encode(&self, limits: &CheckpointLimits) -> CheckpointResult<Vec<u8>> {
        validate_core(self, limits)?;
        let mut output = Vec::with_capacity(core_encoded_len(self, limits)?);
        output.extend_from_slice(&CHECKPOINT_CORE_MAGIC);
        put_u16(&mut output, self.version);
        put_u32(&mut output, self.network_magic);
        put_digest(&mut output, self.genesis_hash);
        put_u32(&mut output, self.height);
        put_digest(&mut output, self.block_hash);
        put_protocol(&mut output, self.protocol);
        put_bytes_u32(&mut output, "signed_state_root", &self.signed_state_root)?;
        put_digest(&mut output, self.state_validators_hash);
        put_stream(&mut output, self.state);
        put_stream(&mut output, self.ledger);
        put_archive(&mut output, self.headers);
        put_archive(&mut output, self.blocks);
        ensure_limit("checkpoint_core", output.len(), limits.max_core_bytes)?;
        Ok(output)
    }

    /// Decodes and validates one complete canonical V1 core.
    pub fn decode(bytes: &[u8], limits: &CheckpointLimits) -> CheckpointResult<Self> {
        ensure_limit("checkpoint_core", bytes.len(), limits.max_core_bytes)?;
        let mut reader = Reader::new(bytes);
        if reader.take("checkpoint_core_magic", CHECKPOINT_CORE_MAGIC.len())?
            != CHECKPOINT_CORE_MAGIC
        {
            return Err(CheckpointError::InvalidMagic {
                kind: "checkpoint core",
            });
        }
        let version = reader.u16("checkpoint_core_version")?;
        ensure_version("checkpoint core", CHECKPOINT_CORE_FORMAT_VERSION, version)?;
        let network_magic = reader.u32("network_magic")?;
        let genesis_hash = reader.digest("genesis_hash")?;
        let height = reader.u32("height")?;
        let block_hash = reader.digest("block_hash")?;
        let protocol = reader.protocol()?;
        let signed_state_root =
            reader.bounded_bytes_u32("signed_state_root", limits.max_core_bytes)?;
        let state_validators_hash = reader.digest("state_validators_hash")?;
        let state = reader.stream("state_stream")?;
        let ledger = reader.stream("ledger_stream")?;
        let headers = reader.archive("header_archive")?;
        let blocks = reader.archive("block_archive")?;
        reader.finish("checkpoint core")?;

        let core = Self {
            version,
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
        };
        validate_core(&core, limits)?;
        Ok(core)
    }

    /// Computes the canonical domain-separated checkpoint ID.
    pub fn checkpoint_id(&self, limits: &CheckpointLimits) -> CheckpointResult<CheckpointId> {
        let bytes = self.encode(limits)?;
        let mut hasher = Sha256::new();
        hasher.update(CHECKPOINT_ID_DOMAIN);
        hasher.update(bytes);
        Ok(CheckpointId::new(digest(hasher.finalize())))
    }
}

impl CheckpointCertificateV1 {
    /// Encodes the detached certificate envelope canonically.
    pub fn encode(&self, limits: &CheckpointLimits) -> CheckpointResult<Vec<u8>> {
        ensure_version(
            "checkpoint certificate",
            CHECKPOINT_CERTIFICATE_FORMAT_VERSION,
            self.version,
        )?;
        if self.witness.is_empty() {
            return Err(CheckpointError::InvalidField {
                field: "certificate_witness",
                reason: "witness is empty",
            });
        }
        ensure_limit(
            "certificate_witness",
            self.witness.len(),
            limits.max_certificate_bytes,
        )?;
        let witness_len =
            u32::try_from(self.witness.len()).map_err(|_| CheckpointError::LimitExceeded {
                field: "certificate_witness",
                actual: u64::MAX,
                maximum: limits.max_certificate_bytes,
            })?;
        let capacity = CHECKPOINT_CERTIFICATE_MAGIC
            .len()
            .checked_add(2 + 32 + 4)
            .and_then(|length| length.checked_add(self.witness.len()))
            .ok_or(CheckpointError::ArithmeticOverflow {
                field: "checkpoint_certificate_bytes",
            })?;
        ensure_limit(
            "checkpoint_certificate",
            capacity,
            limits.max_certificate_bytes,
        )?;
        let mut output = Vec::with_capacity(capacity);
        output.extend_from_slice(&CHECKPOINT_CERTIFICATE_MAGIC);
        put_u16(&mut output, self.version);
        put_digest(&mut output, self.checkpoint_id.digest());
        put_u32(&mut output, witness_len);
        output.extend_from_slice(self.witness.as_slice());
        Ok(output)
    }

    /// Decodes one complete detached certificate envelope.
    pub fn decode(bytes: &[u8], limits: &CheckpointLimits) -> CheckpointResult<Self> {
        ensure_limit(
            "checkpoint_certificate",
            bytes.len(),
            limits.max_certificate_bytes,
        )?;
        let mut reader = Reader::new(bytes);
        if reader.take(
            "checkpoint_certificate_magic",
            CHECKPOINT_CERTIFICATE_MAGIC.len(),
        )? != CHECKPOINT_CERTIFICATE_MAGIC
        {
            return Err(CheckpointError::InvalidMagic {
                kind: "checkpoint certificate",
            });
        }
        let version = reader.u16("checkpoint_certificate_version")?;
        ensure_version(
            "checkpoint certificate",
            CHECKPOINT_CERTIFICATE_FORMAT_VERSION,
            version,
        )?;
        let checkpoint_id = CheckpointId::new(reader.digest("checkpoint_id")?);
        let witness =
            reader.bounded_bytes_u32("certificate_witness", limits.max_certificate_bytes)?;
        if witness.is_empty() {
            return Err(CheckpointError::InvalidField {
                field: "certificate_witness",
                reason: "witness is empty",
            });
        }
        reader.finish("checkpoint certificate")?;
        Ok(Self {
            version,
            checkpoint_id,
            witness,
        })
    }

    /// Returns Neo-style certificate sign data:
    /// `network_magic_le || SHA256(domain || checkpoint_id)`.
    pub fn sign_data(&self, network_magic: u32) -> Vec<u8> {
        checkpoint_certificate_sign_data(network_magic, self.checkpoint_id)
    }
}

/// Computes the hash signed by the supplementary StateValidator certificate.
pub fn checkpoint_certificate_sign_hash(checkpoint_id: CheckpointId) -> Digest32 {
    let mut hasher = Sha256::new();
    hasher.update(CHECKPOINT_CERTIFICATE_SIGN_DOMAIN);
    hasher.update(checkpoint_id.digest().as_bytes());
    digest(hasher.finalize())
}

/// Computes Neo-style sign data for the supplementary certificate.
pub fn checkpoint_certificate_sign_data(
    network_magic: u32,
    checkpoint_id: CheckpointId,
) -> Vec<u8> {
    let mut output = Vec::with_capacity(4 + 32);
    output.extend_from_slice(&network_magic.to_le_bytes());
    output.extend_from_slice(checkpoint_certificate_sign_hash(checkpoint_id).as_bytes());
    output
}

fn validate_core(core: &CheckpointCoreV1, limits: &CheckpointLimits) -> CheckpointResult<()> {
    ensure_version(
        "checkpoint core",
        CHECKPOINT_CORE_FORMAT_VERSION,
        core.version,
    )?;
    if core.signed_state_root.is_empty() {
        return Err(CheckpointError::InvalidField {
            field: "signed_state_root",
            reason: "signed StateRoot is empty",
        });
    }
    ensure_limit(
        "signed_state_root",
        core.signed_state_root.len(),
        limits.max_core_bytes,
    )?;
    validate_stream(core.state, StreamKind::State, limits)?;
    validate_stream(core.ledger, StreamKind::Ledger, limits)?;
    validate_archive(core.headers, StreamKind::Headers, core.height)?;
    validate_archive(core.blocks, StreamKind::Blocks, core.height)?;
    if core.headers.first_height != core.blocks.first_height {
        return Err(CheckpointError::InvalidField {
            field: "archive_first_height",
            reason: "header and block archives start at different heights",
        });
    }
    let total_chunks = core
        .state
        .geometry
        .chunks
        .checked_add(core.ledger.geometry.chunks)
        .and_then(|value| value.checked_add(core.headers.segments))
        .and_then(|value| value.checked_add(core.blocks.segments))
        .ok_or(CheckpointError::ArithmeticOverflow {
            field: "checkpoint_chunks",
        })?;
    if total_chunks > limits.max_chunks {
        return Err(CheckpointError::LimitExceeded {
            field: "checkpoint_chunks",
            actual: total_chunks,
            maximum: limits.max_chunks,
        });
    }
    let total_logical_bytes = core
        .state
        .geometry
        .logical_bytes
        .checked_add(core.ledger.geometry.logical_bytes)
        .and_then(|value| value.checked_add(core.headers.logical_bytes))
        .and_then(|value| value.checked_add(core.blocks.logical_bytes))
        .ok_or(CheckpointError::ArithmeticOverflow {
            field: "checkpoint_logical_bytes",
        })?;
    if total_logical_bytes > limits.max_checkpoint_decoded_bytes {
        return Err(CheckpointError::LimitExceeded {
            field: "checkpoint_logical_bytes",
            actual: total_logical_bytes,
            maximum: limits.max_checkpoint_decoded_bytes,
        });
    }
    Ok(())
}

fn validate_stream(
    stream: StreamCommitmentV1,
    expected: StreamKind,
    limits: &CheckpointLimits,
) -> CheckpointResult<()> {
    if stream.kind != expected {
        return Err(CheckpointError::InvalidField {
            field: "stream_kind",
            reason: "stream appears in the wrong checkpoint-core slot",
        });
    }
    if stream.geometry.rows > limits.max_rows {
        return Err(CheckpointError::LimitExceeded {
            field: "stream_rows",
            actual: stream.geometry.rows,
            maximum: limits.max_rows,
        });
    }
    if stream.geometry.chunks > limits.max_chunks {
        return Err(CheckpointError::LimitExceeded {
            field: "stream_chunks",
            actual: stream.geometry.chunks,
            maximum: limits.max_chunks,
        });
    }
    if stream.geometry.rows == 0 {
        if stream.geometry.chunks != 0
            || stream.geometry.key_bytes != 0
            || stream.geometry.value_bytes != 0
            || stream.geometry.logical_bytes != 0
        {
            return Err(CheckpointError::InvalidField {
                field: "stream_geometry",
                reason: "empty stream has nonzero geometry",
            });
        }
        return Ok(());
    }
    if stream.geometry.chunks == 0 {
        return Err(CheckpointError::InvalidField {
            field: "stream_chunks",
            reason: "nonempty stream has no chunks",
        });
    }
    if stream.geometry.key_bytes < stream.geometry.rows {
        return Err(CheckpointError::InvalidField {
            field: "stream_key_bytes",
            reason: "nonempty rows require nonempty raw keys",
        });
    }
    let framing_bytes = stream
        .geometry
        .rows
        .checked_mul(CHECKPOINT_ROW_FIXED_BYTES)
        .ok_or(CheckpointError::ArithmeticOverflow {
            field: "stream_framing_bytes",
        })?;
    let expected_logical_bytes = framing_bytes
        .checked_add(stream.geometry.key_bytes)
        .and_then(|value| value.checked_add(stream.geometry.value_bytes))
        .ok_or(CheckpointError::ArithmeticOverflow {
            field: "stream_logical_bytes",
        })?;
    if stream.geometry.logical_bytes != expected_logical_bytes {
        return Err(CheckpointError::InvalidField {
            field: "stream_logical_bytes",
            reason: "logical bytes do not match row framing plus key/value bytes",
        });
    }
    if stream.geometry.logical_bytes > limits.max_checkpoint_decoded_bytes {
        return Err(CheckpointError::LimitExceeded {
            field: "stream_logical_bytes",
            actual: stream.geometry.logical_bytes,
            maximum: limits.max_checkpoint_decoded_bytes,
        });
    }
    Ok(())
}

fn validate_archive(
    archive: ArchiveCommitmentV1,
    expected: StreamKind,
    checkpoint_height: u32,
) -> CheckpointResult<()> {
    if archive.kind != expected {
        return Err(CheckpointError::InvalidField {
            field: "archive_kind",
            reason: "archive appears in the wrong checkpoint-core slot",
        });
    }
    if archive.first_height > archive.last_height {
        return Err(CheckpointError::InvalidField {
            field: "archive_height_range",
            reason: "archive starts after it ends",
        });
    }
    if archive.first_height != 0 {
        return Err(CheckpointError::InvalidField {
            field: "archive_first_height",
            reason: "V1 archive does not start at genesis",
        });
    }
    if archive.last_height != checkpoint_height {
        return Err(CheckpointError::InvalidField {
            field: "archive_last_height",
            reason: "archive does not end at the checkpoint height",
        });
    }
    let expected_records = u64::from(checkpoint_height) + 1;
    if archive.records != expected_records || archive.segments == 0 || archive.logical_bytes == 0 {
        return Err(CheckpointError::InvalidField {
            field: "archive_geometry",
            reason: "archive does not contain complete genesis-through-H records",
        });
    }
    Ok(())
}

fn core_encoded_len(core: &CheckpointCoreV1, limits: &CheckpointLimits) -> CheckpointResult<usize> {
    // Fixed fields: magic/version/network/genesis/height/block/protocol,
    // StateRoot length, validator hash, two streams, and two archives.
    const FIXED: usize = 8
        + 2
        + 4
        + 32
        + 4
        + 32
        + (5 * 32)
        + 4
        + 32
        + (2 * (1 + 32 + (5 * 8)))
        + (2 * (1 + 4 + 4 + (3 * 8) + 32));
    let length = FIXED.checked_add(core.signed_state_root.len()).ok_or(
        CheckpointError::ArithmeticOverflow {
            field: "checkpoint_core_bytes",
        },
    )?;
    ensure_limit("checkpoint_core", length, limits.max_core_bytes)?;
    Ok(length)
}

pub(crate) fn ensure_version(
    kind: &'static str,
    expected: u16,
    actual: u16,
) -> CheckpointResult<()> {
    if actual != expected {
        return Err(CheckpointError::UnsupportedVersion {
            kind,
            expected,
            actual,
        });
    }
    Ok(())
}

pub(crate) fn ensure_limit(
    field: &'static str,
    actual: usize,
    maximum: u64,
) -> CheckpointResult<()> {
    let actual = u64::try_from(actual).map_err(|_| CheckpointError::LimitExceeded {
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
    Ok(())
}

pub(crate) fn digest(bytes: impl AsRef<[u8]>) -> Digest32 {
    let mut output = [0u8; 32];
    output.copy_from_slice(bytes.as_ref());
    Digest32::from_bytes(output)
}

pub(crate) fn put_u16(output: &mut Vec<u8>, value: u16) {
    output.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_u32(output: &mut Vec<u8>, value: u32) {
    output.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_u64(output: &mut Vec<u8>, value: u64) {
    output.extend_from_slice(&value.to_le_bytes());
}

pub(crate) fn put_digest(output: &mut Vec<u8>, value: Digest32) {
    output.extend_from_slice(value.as_bytes());
}

pub(crate) fn put_bytes_u32(
    output: &mut Vec<u8>,
    field: &'static str,
    value: &BoundedBytes,
) -> CheckpointResult<()> {
    let length = u32::try_from(value.len()).map_err(|_| CheckpointError::LimitExceeded {
        field,
        actual: u64::MAX,
        maximum: u64::from(u32::MAX),
    })?;
    put_u32(output, length);
    output.extend_from_slice(value.as_slice());
    Ok(())
}

fn put_protocol(output: &mut Vec<u8>, protocol: ProtocolIdentityV1) {
    put_digest(output, protocol.protocol_settings);
    put_digest(output, protocol.hardforks);
    put_digest(output, protocol.native_contracts);
    put_digest(output, protocol.neo_vm);
    put_digest(output, protocol.mpt_codec);
}

fn put_stream(output: &mut Vec<u8>, stream: StreamCommitmentV1) {
    output.push(stream.kind as u8);
    put_digest(output, stream.root);
    put_u64(output, stream.geometry.rows);
    put_u64(output, stream.geometry.chunks);
    put_u64(output, stream.geometry.key_bytes);
    put_u64(output, stream.geometry.value_bytes);
    put_u64(output, stream.geometry.logical_bytes);
}

fn put_archive(output: &mut Vec<u8>, archive: ArchiveCommitmentV1) {
    output.push(archive.kind as u8);
    put_u32(output, archive.first_height);
    put_u32(output, archive.last_height);
    put_u64(output, archive.segments);
    put_u64(output, archive.records);
    put_u64(output, archive.logical_bytes);
    put_digest(output, archive.root);
}

pub(crate) struct Reader<'a> {
    bytes: &'a [u8],
    offset: usize,
}

impl<'a> Reader<'a> {
    pub(crate) const fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, offset: 0 }
    }

    pub(crate) fn take(
        &mut self,
        field: &'static str,
        length: usize,
    ) -> CheckpointResult<&'a [u8]> {
        let remaining = self.bytes.len().saturating_sub(self.offset);
        if length > remaining {
            return Err(CheckpointError::UnexpectedEof {
                field,
                needed: u64::try_from(length).unwrap_or(u64::MAX),
                remaining: u64::try_from(remaining).unwrap_or(u64::MAX),
            });
        }
        let end = self
            .offset
            .checked_add(length)
            .ok_or(CheckpointError::ArithmeticOverflow { field })?;
        let value = &self.bytes[self.offset..end];
        self.offset = end;
        Ok(value)
    }

    pub(crate) fn u8(&mut self, field: &'static str) -> CheckpointResult<u8> {
        Ok(self.take(field, 1)?[0])
    }

    pub(crate) fn u16(&mut self, field: &'static str) -> CheckpointResult<u16> {
        let bytes: [u8; 2] = self.take(field, 2)?.try_into().expect("exact length");
        Ok(u16::from_le_bytes(bytes))
    }

    pub(crate) fn u32(&mut self, field: &'static str) -> CheckpointResult<u32> {
        let bytes: [u8; 4] = self.take(field, 4)?.try_into().expect("exact length");
        Ok(u32::from_le_bytes(bytes))
    }

    pub(crate) fn u64(&mut self, field: &'static str) -> CheckpointResult<u64> {
        let bytes: [u8; 8] = self.take(field, 8)?.try_into().expect("exact length");
        Ok(u64::from_le_bytes(bytes))
    }

    pub(crate) fn digest(&mut self, field: &'static str) -> CheckpointResult<Digest32> {
        let bytes: [u8; 32] = self.take(field, 32)?.try_into().expect("exact length");
        Ok(Digest32::from_bytes(bytes))
    }

    pub(crate) fn bounded_bytes_u32(
        &mut self,
        field: &'static str,
        maximum: u64,
    ) -> CheckpointResult<BoundedBytes> {
        let length = self.u32(field)?;
        if u64::from(length) > maximum {
            return Err(CheckpointError::LimitExceeded {
                field,
                actual: u64::from(length),
                maximum,
            });
        }
        BoundedBytes::try_new(field, self.take(field, length as usize)?.to_vec(), maximum)
    }

    fn protocol(&mut self) -> CheckpointResult<ProtocolIdentityV1> {
        Ok(ProtocolIdentityV1 {
            protocol_settings: self.digest("protocol_settings_digest")?,
            hardforks: self.digest("hardfork_digest")?,
            native_contracts: self.digest("native_contracts_digest")?,
            neo_vm: self.digest("neo_vm_digest")?,
            mpt_codec: self.digest("mpt_codec_digest")?,
        })
    }

    fn stream(&mut self, field: &'static str) -> CheckpointResult<StreamCommitmentV1> {
        let kind = stream_kind(field, self.u8(field)?)?;
        Ok(StreamCommitmentV1 {
            kind,
            root: self.digest(field)?,
            geometry: StreamGeometryV1 {
                rows: self.u64(field)?,
                chunks: self.u64(field)?,
                key_bytes: self.u64(field)?,
                value_bytes: self.u64(field)?,
                logical_bytes: self.u64(field)?,
            },
        })
    }

    fn archive(&mut self, field: &'static str) -> CheckpointResult<ArchiveCommitmentV1> {
        let kind = stream_kind(field, self.u8(field)?)?;
        Ok(ArchiveCommitmentV1 {
            kind,
            first_height: self.u32(field)?,
            last_height: self.u32(field)?,
            segments: self.u64(field)?,
            records: self.u64(field)?,
            logical_bytes: self.u64(field)?,
            root: self.digest(field)?,
        })
    }

    pub(crate) fn finish(self, kind: &'static str) -> CheckpointResult<()> {
        let trailing = self.bytes.len().saturating_sub(self.offset);
        if trailing != 0 {
            return Err(CheckpointError::TrailingBytes {
                kind,
                trailing: u64::try_from(trailing).unwrap_or(u64::MAX),
            });
        }
        Ok(())
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    fn hash(byte: u8) -> Digest32 {
        Digest32::from_bytes([byte; 32])
    }

    fn stream(kind: StreamKind, byte: u8) -> StreamCommitmentV1 {
        StreamCommitmentV1 {
            kind,
            root: hash(byte),
            geometry: StreamGeometryV1 {
                rows: 10,
                chunks: 2,
                key_bytes: 20,
                value_bytes: 30,
                logical_bytes: 320,
            },
        }
    }

    fn archive(kind: StreamKind, byte: u8) -> ArchiveCommitmentV1 {
        ArchiveCommitmentV1 {
            kind,
            first_height: 0,
            last_height: 42,
            segments: 2,
            records: 43,
            logical_bytes: 1_000,
            root: hash(byte),
        }
    }

    fn core() -> CheckpointCoreV1 {
        CheckpointCoreV1::v1(
            0x334f_454e,
            hash(1),
            42,
            hash(2),
            ProtocolIdentityV1 {
                protocol_settings: hash(3),
                hardforks: hash(4),
                native_contracts: hash(5),
                neo_vm: hash(6),
                mpt_codec: hash(7),
            },
            BoundedBytes::try_new("signed_state_root", vec![8, 9, 10], 16).unwrap(),
            hash(11),
            stream(StreamKind::State, 12),
            stream(StreamKind::Ledger, 13),
            archive(StreamKind::Headers, 14),
            archive(StreamKind::Blocks, 15),
        )
    }

    #[test]
    fn checkpoint_core_round_trips_canonical_bytes() {
        let limits = CheckpointLimits::default();
        let core = core();
        let encoded = core.encode(&limits).unwrap();
        assert_eq!(&encoded[..8], &CHECKPOINT_CORE_MAGIC);
        assert_eq!(encoded.len(), core_encoded_len(&core, &limits).unwrap());
        assert_eq!(CheckpointCoreV1::decode(&encoded, &limits).unwrap(), core);
        assert_eq!(
            CheckpointCoreV1::decode(&encoded, &limits)
                .unwrap()
                .encode(&limits)
                .unwrap(),
            encoded
        );
    }

    #[test]
    fn checkpoint_id_changes_with_any_core_byte() {
        let limits = CheckpointLimits::default();
        let original = core();
        let mut changed = original.clone();
        changed.block_hash = hash(99);
        assert_ne!(
            original.checkpoint_id(&limits).unwrap(),
            changed.checkpoint_id(&limits).unwrap()
        );
    }

    #[test]
    fn decoder_rejects_magic_version_truncation_and_trailing_bytes() {
        let limits = CheckpointLimits::default();
        let encoded = core().encode(&limits).unwrap();

        let mut wrong_magic = encoded.clone();
        wrong_magic[0] ^= 1;
        assert!(matches!(
            CheckpointCoreV1::decode(&wrong_magic, &limits),
            Err(CheckpointError::InvalidMagic { .. })
        ));

        let mut wrong_version = encoded.clone();
        wrong_version[8..10].copy_from_slice(&2u16.to_le_bytes());
        assert!(matches!(
            CheckpointCoreV1::decode(&wrong_version, &limits),
            Err(CheckpointError::UnsupportedVersion { .. })
        ));

        assert!(matches!(
            CheckpointCoreV1::decode(&encoded[..encoded.len() - 1], &limits),
            Err(CheckpointError::UnexpectedEof { .. })
        ));

        let mut trailing = encoded;
        trailing.push(0);
        assert!(matches!(
            CheckpointCoreV1::decode(&trailing, &limits),
            Err(CheckpointError::TrailingBytes { .. })
        ));
    }

    #[test]
    fn certificate_round_trips_and_sign_data_binds_network() {
        let limits = CheckpointLimits::default();
        let id = core().checkpoint_id(&limits).unwrap();
        let certificate = CheckpointCertificateV1::v1(
            id,
            BoundedBytes::try_new("witness", vec![1, 2, 3], 16).unwrap(),
        );
        let bytes = certificate.encode(&limits).unwrap();
        assert_eq!(
            CheckpointCertificateV1::decode(&bytes, &limits).unwrap(),
            certificate
        );
        assert_eq!(certificate.sign_data(1).len(), 36);
        assert_ne!(certificate.sign_data(1), certificate.sign_data(2));
        assert_eq!(&certificate.sign_data(1)[..4], &1u32.to_le_bytes());
    }

    #[test]
    fn core_rejects_wrong_slots_and_archive_horizon() {
        let limits = CheckpointLimits::default();
        let mut wrong_stream = core();
        wrong_stream.state.kind = StreamKind::Ledger;
        assert!(wrong_stream.encode(&limits).is_err());

        let mut wrong_horizon = core();
        wrong_horizon.blocks.last_height = 41;
        assert!(wrong_horizon.encode(&limits).is_err());
    }
}
