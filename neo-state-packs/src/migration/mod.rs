//! # Offline node-pack migration stream
//!
//! Deterministic, bounded decoding for the one-time neutral stream used to
//! move an exact live StateService node namespace between pack format
//! generations.
//!
//! ## Boundary
//!
//! This module owns only the versioned interchange bytes and their integrity
//! checks. It does not open MDBX, build pack frames, publish checkpoint
//! metadata, or activate authoritative storage.
//!
//! ## Contents
//!
//! - [`MigrationStreamReader`]: streaming decoder with exact geometry and
//!   digest validation.
//! - [`MigrationStreamHeader`]: authenticated source identity and namespace
//!   geometry.
//! - [`MigrationStreamLimits`]: hard allocation and input bounds.

use std::fmt;
use std::io::{self, Read};

use sha2::{Digest, Sha256};
use thiserror::Error;

use crate::{CHECKPOINT_NAMESPACE_DIGEST_DOMAIN, PACK_KEY_BYTES};

/// Neutral migration stream format version.
pub const MIGRATION_STREAM_FORMAT_VERSION: u32 = 1;
/// Fixed v1 header length.
pub const MIGRATION_STREAM_HEADER_BYTES: usize = 160;
/// Fixed v1 trailer length.
pub const MIGRATION_STREAM_TRAILER_BYTES: usize = 96;
/// Maximum value accepted by the neutral v1 contract.
pub const MIGRATION_STREAM_MAX_VALUE_BYTES: u64 = 1024 * 1024;

const HEADER_MAGIC: &[u8; 8] = b"N3MIGR01";
const TRAILER_MAGIC: &[u8; 8] = b"N3MIGEND";
const PAYLOAD_DIGEST_DOMAIN: &[u8] = b"neo-state-pack-migration/payload/v1\0";
const STREAM_DIGEST_DOMAIN: &[u8] = b"neo-state-pack-migration/stream/v1\0";
const ROW_PREFIX_BYTES: u64 = PACK_KEY_BYTES as u64 + size_of::<u32>() as u64;

/// Result returned by neutral migration stream decoding.
pub type MigrationStreamResult<T> = Result<T, MigrationStreamError>;

/// Stable resource names used by [`MigrationStreamError::LimitExceeded`].
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum MigrationStreamLimit {
    /// Total encoded stream bytes.
    StreamBytes,
    /// Live namespace row count.
    Rows,
    /// One exact node value.
    ValueBytes,
}

impl fmt::Display for MigrationStreamLimit {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::StreamBytes => "stream bytes",
            Self::Rows => "rows",
            Self::ValueBytes => "value bytes",
        })
    }
}

/// Typed failures returned while decoding an offline migration stream.
#[derive(Debug, Error)]
pub enum MigrationStreamError {
    /// A fixed stream region could not be read completely.
    #[error("read migration stream {region}: {source}")]
    Io {
        /// Region being read.
        region: &'static str,
        /// Underlying I/O failure.
        #[source]
        source: io::Error,
    },
    /// The stream uses a format version this binary cannot decode.
    #[error(
        "unsupported migration stream version {found}; expected {MIGRATION_STREAM_FORMAT_VERSION}"
    )]
    UnsupportedVersion {
        /// Encoded version.
        found: u32,
    },
    /// A structural field violates the deterministic v1 contract.
    #[error("invalid migration stream {field}: {message}")]
    InvalidFormat {
        /// Field or region that failed validation.
        field: &'static str,
        /// Stable diagnostic detail.
        message: String,
    },
    /// A declared or observed resource exceeds a configured bound.
    #[error("migration stream {limit} {actual} exceeds limit {maximum}")]
    LimitExceeded {
        /// Bounded resource.
        limit: MigrationStreamLimit,
        /// Declared or observed size.
        actual: u64,
        /// Accepted maximum.
        maximum: u64,
    },
    /// An authenticated digest does not match the bytes that were read.
    #[error("migration stream {digest} SHA-256 mismatch")]
    DigestMismatch {
        /// Digest field that did not match.
        digest: &'static str,
    },
    /// The caller attempted to finish before consuming every declared row.
    #[error("migration stream is incomplete: read {read_rows} of {expected_rows} rows")]
    Incomplete {
        /// Rows returned to the caller.
        read_rows: u64,
        /// Rows declared by the header.
        expected_rows: u64,
    },
}

/// Decoder bounds applied before allocation from persisted lengths.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationStreamLimits {
    max_stream_bytes: u64,
    max_rows: u64,
    max_value_bytes: u64,
}

impl MigrationStreamLimits {
    /// Maximum total stream size accepted by the default offline tool.
    pub const DEFAULT_MAX_STREAM_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
    /// Maximum live rows accepted by the default offline tool.
    pub const DEFAULT_MAX_ROWS: u64 = 1_000_000_000;

    /// Creates validated decoder bounds.
    pub fn new(
        max_stream_bytes: u64,
        max_rows: u64,
        max_value_bytes: u64,
    ) -> MigrationStreamResult<Self> {
        ensure_nonzero("maximum stream bytes", max_stream_bytes)?;
        ensure_nonzero("maximum rows", max_rows)?;
        ensure_nonzero("maximum value bytes", max_value_bytes)?;
        if max_value_bytes > MIGRATION_STREAM_MAX_VALUE_BYTES {
            return Err(MigrationStreamError::LimitExceeded {
                limit: MigrationStreamLimit::ValueBytes,
                actual: max_value_bytes,
                maximum: MIGRATION_STREAM_MAX_VALUE_BYTES,
            });
        }
        Ok(Self {
            max_stream_bytes,
            max_rows,
            max_value_bytes,
        })
    }

    /// Returns the maximum complete encoded stream length.
    pub const fn max_stream_bytes(self) -> u64 {
        self.max_stream_bytes
    }

    /// Returns the maximum row count.
    pub const fn max_rows(self) -> u64 {
        self.max_rows
    }

    /// Returns the maximum bytes accepted for one value.
    pub const fn max_value_bytes(self) -> u64 {
        self.max_value_bytes
    }
}

impl Default for MigrationStreamLimits {
    fn default() -> Self {
        Self {
            max_stream_bytes: Self::DEFAULT_MAX_STREAM_BYTES,
            max_rows: Self::DEFAULT_MAX_ROWS,
            max_value_bytes: MIGRATION_STREAM_MAX_VALUE_BYTES,
        }
    }
}

/// Authenticated source identity and exact payload geometry from a v1 header.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationStreamHeader {
    /// Neo network magic of the source checkpoint.
    pub network_magic: u32,
    /// StateService checkpoint height.
    pub height: u32,
    /// Internal little-endian StateService root bytes.
    pub root_internal: [u8; 32],
    /// Number of unique sorted live rows.
    pub rows: u64,
    /// Sum of exact value lengths.
    pub value_bytes: u64,
    /// Bytes occupied by all row prefixes and values.
    pub payload_bytes: u64,
    /// Domain-separated digest of the materialized live namespace.
    pub namespace_sha256: [u8; 32],
    /// Domain-separated digest of the raw payload bytes.
    pub payload_sha256: [u8; 32],
}

/// One exact live namespace row decoded from a migration stream.
#[derive(Debug, Eq, PartialEq)]
pub struct MigrationStreamRow {
    /// Strictly increasing `0xf0 || node_hash` key.
    pub key: [u8; PACK_KEY_BYTES],
    /// Exact serialized StateService MPT node value.
    pub value: Vec<u8>,
}

/// Evidence produced only after header, payload, trailer, and EOF validation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct MigrationStreamEvidence {
    /// Validated source header.
    pub header: MigrationStreamHeader,
    /// Domain-separated digest binding the final header and raw payload.
    pub stream_sha256: [u8; 32],
}

/// Streaming decoder for a deterministic neutral v1 migration stream.
pub struct MigrationStreamReader<R> {
    reader: R,
    header: MigrationStreamHeader,
    payload_hasher: Sha256,
    namespace_hasher: Sha256,
    stream_hasher: Sha256,
    previous_key: Option<[u8; PACK_KEY_BYTES]>,
    rows_read: u64,
    value_bytes_read: u64,
    payload_bytes_read: u64,
    max_value_bytes: u64,
    evidence: Option<MigrationStreamEvidence>,
}

impl<R: Read> MigrationStreamReader<R> {
    /// Reads and validates a v1 header without allocating from its lengths.
    pub fn new(mut reader: R, limits: MigrationStreamLimits) -> MigrationStreamResult<Self> {
        let mut encoded_header = [0u8; MIGRATION_STREAM_HEADER_BYTES];
        read_exact(&mut reader, &mut encoded_header, "header")?;
        let header = decode_header(&encoded_header, limits)?;

        let mut payload_hasher = Sha256::new();
        payload_hasher.update(PAYLOAD_DIGEST_DOMAIN);
        let mut namespace_hasher = Sha256::new();
        namespace_hasher.update(CHECKPOINT_NAMESPACE_DIGEST_DOMAIN);
        let mut stream_hasher = Sha256::new();
        stream_hasher.update(STREAM_DIGEST_DOMAIN);
        stream_hasher.update(encoded_header);

        Ok(Self {
            reader,
            header,
            payload_hasher,
            namespace_hasher,
            stream_hasher,
            previous_key: None,
            rows_read: 0,
            value_bytes_read: 0,
            payload_bytes_read: 0,
            max_value_bytes: limits.max_value_bytes,
            evidence: None,
        })
    }

    /// Returns the validated source header.
    pub const fn header(&self) -> MigrationStreamHeader {
        self.header
    }

    /// Decodes the next exact live row, or validates the trailer and returns
    /// `None` after all declared rows have been consumed.
    pub fn read_row(&mut self) -> MigrationStreamResult<Option<MigrationStreamRow>> {
        if self.rows_read == self.header.rows {
            if self.evidence.is_none() {
                self.validate_trailer_and_eof()?;
            }
            return Ok(None);
        }

        let mut prefix = [0u8; PACK_KEY_BYTES + size_of::<u32>()];
        read_exact(&mut self.reader, &mut prefix, "row prefix")?;
        let key: [u8; PACK_KEY_BYTES] = prefix[..PACK_KEY_BYTES]
            .try_into()
            .expect("fixed key field");
        if key[0] != 0xf0 {
            return Err(invalid_row(
                self.rows_read,
                "key is outside the 0xf0 namespace",
            ));
        }
        if self.previous_key.is_some_and(|previous| previous >= key) {
            return Err(invalid_row(
                self.rows_read,
                "keys are not strictly increasing and unique",
            ));
        }
        let value_len = u32::from_le_bytes(
            prefix[PACK_KEY_BYTES..]
                .try_into()
                .expect("fixed value-length field"),
        );
        let value_len = u64::from(value_len);
        if value_len > self.max_value_bytes {
            return Err(MigrationStreamError::LimitExceeded {
                limit: MigrationStreamLimit::ValueBytes,
                actual: value_len,
                maximum: self.max_value_bytes,
            });
        }
        let next_value_bytes = self
            .value_bytes_read
            .checked_add(value_len)
            .ok_or_else(|| invalid_row(self.rows_read, "value-byte count overflows u64"))?;
        if next_value_bytes > self.header.value_bytes {
            return Err(invalid_row(
                self.rows_read,
                "values exceed the byte count declared by the header",
            ));
        }
        let row_bytes = ROW_PREFIX_BYTES
            .checked_add(value_len)
            .ok_or_else(|| invalid_row(self.rows_read, "row byte count overflows u64"))?;
        let next_payload_bytes = self
            .payload_bytes_read
            .checked_add(row_bytes)
            .ok_or_else(|| invalid_row(self.rows_read, "payload byte count overflows u64"))?;
        if next_payload_bytes > self.header.payload_bytes {
            return Err(invalid_row(
                self.rows_read,
                "row exceeds the payload length declared by the header",
            ));
        }

        let value_len = usize::try_from(value_len)
            .map_err(|_| invalid_row(self.rows_read, "value length overflows usize"))?;
        let mut value = Vec::new();
        value.try_reserve_exact(value_len).map_err(|error| {
            MigrationStreamError::InvalidFormat {
                field: "row value",
                message: format!("cannot reserve {value_len} bytes: {error}"),
            }
        })?;
        value.resize(value_len, 0);
        read_exact(&mut self.reader, &mut value, "row value")?;

        self.payload_hasher.update(prefix);
        self.payload_hasher.update(&value);
        self.stream_hasher.update(prefix);
        self.stream_hasher.update(&value);
        self.namespace_hasher
            .update((PACK_KEY_BYTES as u32).to_le_bytes());
        self.namespace_hasher.update(key);
        self.namespace_hasher
            .update((value_len as u64).to_le_bytes());
        self.namespace_hasher.update(&value);
        self.previous_key = Some(key);
        self.rows_read += 1;
        self.value_bytes_read = next_value_bytes;
        self.payload_bytes_read = next_payload_bytes;
        Ok(Some(MigrationStreamRow { key, value }))
    }

    /// Finishes the decoder after the caller has consumed every row.
    ///
    /// This method also validates the trailer when the caller did not make a
    /// final `read_row` call that returned `None`.
    pub fn finish(mut self) -> MigrationStreamResult<MigrationStreamEvidence> {
        if self.rows_read != self.header.rows {
            return Err(MigrationStreamError::Incomplete {
                read_rows: self.rows_read,
                expected_rows: self.header.rows,
            });
        }
        if self.evidence.is_none() {
            self.validate_trailer_and_eof()?;
        }
        Ok(self.evidence.expect("validated trailer installs evidence"))
    }

    fn validate_trailer_and_eof(&mut self) -> MigrationStreamResult<()> {
        if self.value_bytes_read != self.header.value_bytes
            || self.payload_bytes_read != self.header.payload_bytes
        {
            return Err(MigrationStreamError::InvalidFormat {
                field: "payload geometry",
                message: format!(
                    "observed {} value and {} payload bytes, expected {} and {}",
                    self.value_bytes_read,
                    self.payload_bytes_read,
                    self.header.value_bytes,
                    self.header.payload_bytes,
                ),
            });
        }
        let namespace_sha256: [u8; 32] = self.namespace_hasher.clone().finalize().into();
        if namespace_sha256 != self.header.namespace_sha256 {
            return Err(MigrationStreamError::DigestMismatch {
                digest: "namespace",
            });
        }
        let payload_sha256: [u8; 32] = self.payload_hasher.clone().finalize().into();
        if payload_sha256 != self.header.payload_sha256 {
            return Err(MigrationStreamError::DigestMismatch { digest: "payload" });
        }

        let mut trailer = [0u8; MIGRATION_STREAM_TRAILER_BYTES];
        read_exact(&mut self.reader, &mut trailer, "trailer")?;
        validate_trailer_fields(&trailer)?;
        let expected_stream_sha256: [u8; 32] = trailer[16..48]
            .try_into()
            .expect("fixed stream digest field");
        let trailer_namespace_sha256: [u8; 32] = trailer[48..80]
            .try_into()
            .expect("fixed namespace digest field");
        if trailer_namespace_sha256 != namespace_sha256 {
            return Err(MigrationStreamError::DigestMismatch {
                digest: "trailer namespace",
            });
        }
        let stream_sha256: [u8; 32] = self.stream_hasher.clone().finalize().into();
        if stream_sha256 != expected_stream_sha256 {
            return Err(MigrationStreamError::DigestMismatch { digest: "stream" });
        }
        let mut trailing = [0u8; 1];
        match self.reader.read(&mut trailing) {
            Ok(0) => {}
            Ok(_) => {
                return Err(MigrationStreamError::InvalidFormat {
                    field: "end of file",
                    message: "bytes follow the v1 trailer".to_owned(),
                });
            }
            Err(source) => {
                return Err(MigrationStreamError::Io {
                    region: "end of file",
                    source,
                });
            }
        }
        self.evidence = Some(MigrationStreamEvidence {
            header: self.header,
            stream_sha256,
        });
        Ok(())
    }
}

fn decode_header(
    header: &[u8; MIGRATION_STREAM_HEADER_BYTES],
    limits: MigrationStreamLimits,
) -> MigrationStreamResult<MigrationStreamHeader> {
    if &header[..8] != HEADER_MAGIC {
        return Err(invalid_format("header magic", "expected N3MIGR01"));
    }
    let version = u32_at(header, 8);
    if version != MIGRATION_STREAM_FORMAT_VERSION {
        return Err(MigrationStreamError::UnsupportedVersion { found: version });
    }
    if u32_at(header, 12) as usize != MIGRATION_STREAM_HEADER_BYTES {
        return Err(invalid_format("header length", "expected 160 bytes"));
    }
    if u32_at(header, 20) != 0 || u32_at(header, 28) != 0 {
        return Err(invalid_format(
            "header flags",
            "flags and reserved must be zero",
        ));
    }
    if header[152..].iter().any(|byte| *byte != 0) {
        return Err(invalid_format(
            "header reserved",
            "reserved bytes must be zero",
        ));
    }
    let rows = u64_at(header, 64);
    if rows > limits.max_rows {
        return Err(MigrationStreamError::LimitExceeded {
            limit: MigrationStreamLimit::Rows,
            actual: rows,
            maximum: limits.max_rows,
        });
    }
    let value_bytes = u64_at(header, 72);
    let payload_bytes = u64_at(header, 80);
    let expected_payload_bytes = rows
        .checked_mul(ROW_PREFIX_BYTES)
        .and_then(|bytes| bytes.checked_add(value_bytes))
        .ok_or_else(|| invalid_format("payload length", "declared geometry overflows u64"))?;
    if payload_bytes != expected_payload_bytes {
        return Err(invalid_format(
            "payload length",
            format!(
                "declares {payload_bytes} bytes, expected {expected_payload_bytes} from rows and values"
            ),
        ));
    }
    let stream_bytes = (MIGRATION_STREAM_HEADER_BYTES as u64)
        .checked_add(payload_bytes)
        .and_then(|bytes| bytes.checked_add(MIGRATION_STREAM_TRAILER_BYTES as u64))
        .ok_or_else(|| invalid_format("stream length", "declared geometry overflows u64"))?;
    if stream_bytes > limits.max_stream_bytes {
        return Err(MigrationStreamError::LimitExceeded {
            limit: MigrationStreamLimit::StreamBytes,
            actual: stream_bytes,
            maximum: limits.max_stream_bytes,
        });
    }
    Ok(MigrationStreamHeader {
        network_magic: u32_at(header, 16),
        height: u32_at(header, 24),
        root_internal: header[32..64].try_into().expect("fixed root field"),
        rows,
        value_bytes,
        payload_bytes,
        namespace_sha256: header[88..120]
            .try_into()
            .expect("fixed namespace digest field"),
        payload_sha256: header[120..152]
            .try_into()
            .expect("fixed payload digest field"),
    })
}

fn validate_trailer_fields(
    trailer: &[u8; MIGRATION_STREAM_TRAILER_BYTES],
) -> MigrationStreamResult<()> {
    if &trailer[..8] != TRAILER_MAGIC {
        return Err(invalid_format("trailer magic", "expected N3MIGEND"));
    }
    let version = u32_at(trailer, 8);
    if version != MIGRATION_STREAM_FORMAT_VERSION {
        return Err(MigrationStreamError::UnsupportedVersion { found: version });
    }
    if u32_at(trailer, 12) as usize != MIGRATION_STREAM_TRAILER_BYTES {
        return Err(invalid_format("trailer length", "expected 96 bytes"));
    }
    if trailer[80..].iter().any(|byte| *byte != 0) {
        return Err(invalid_format(
            "trailer reserved",
            "reserved bytes must be zero",
        ));
    }
    Ok(())
}

fn read_exact(
    reader: &mut impl Read,
    bytes: &mut [u8],
    region: &'static str,
) -> MigrationStreamResult<()> {
    reader
        .read_exact(bytes)
        .map_err(|source| MigrationStreamError::Io { region, source })
}

fn u32_at(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(
        bytes[offset..offset + 4]
            .try_into()
            .expect("fixed u32 field"),
    )
}

fn u64_at(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(
        bytes[offset..offset + 8]
            .try_into()
            .expect("fixed u64 field"),
    )
}

fn ensure_nonzero(field: &'static str, value: u64) -> MigrationStreamResult<()> {
    if value == 0 {
        Err(invalid_format(field, "must be greater than zero"))
    } else {
        Ok(())
    }
}

fn invalid_row(index: u64, message: impl Into<String>) -> MigrationStreamError {
    invalid_format("row", format!("row {index}: {}", message.into()))
}

fn invalid_format(field: &'static str, message: impl Into<String>) -> MigrationStreamError {
    MigrationStreamError::InvalidFormat {
        field,
        message: message.into(),
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn key(suffix: u8) -> [u8; PACK_KEY_BYTES] {
        let mut key = [suffix; PACK_KEY_BYTES];
        key[0] = 0xf0;
        key
    }

    fn encode_stream(rows: &[([u8; PACK_KEY_BYTES], &[u8])]) -> Vec<u8> {
        let mut payload = Vec::new();
        let mut namespace = Sha256::new();
        namespace.update(CHECKPOINT_NAMESPACE_DIGEST_DOMAIN);
        for (key, value) in rows {
            payload.extend_from_slice(key);
            payload.extend_from_slice(&(value.len() as u32).to_le_bytes());
            payload.extend_from_slice(value);
            namespace.update((PACK_KEY_BYTES as u32).to_le_bytes());
            namespace.update(key);
            namespace.update((value.len() as u64).to_le_bytes());
            namespace.update(value);
        }
        let namespace_sha256: [u8; 32] = namespace.finalize().into();
        let mut payload_hasher = Sha256::new();
        payload_hasher.update(PAYLOAD_DIGEST_DOMAIN);
        payload_hasher.update(&payload);
        let payload_sha256: [u8; 32] = payload_hasher.finalize().into();

        let mut header = [0u8; MIGRATION_STREAM_HEADER_BYTES];
        header[..8].copy_from_slice(HEADER_MAGIC);
        header[8..12].copy_from_slice(&MIGRATION_STREAM_FORMAT_VERSION.to_le_bytes());
        header[12..16].copy_from_slice(&(MIGRATION_STREAM_HEADER_BYTES as u32).to_le_bytes());
        header[16..20].copy_from_slice(&0x334f_454eu32.to_le_bytes());
        header[24..28].copy_from_slice(&123u32.to_le_bytes());
        header[32..64].copy_from_slice(&[0x55; 32]);
        header[64..72].copy_from_slice(&(rows.len() as u64).to_le_bytes());
        let value_bytes = rows
            .iter()
            .map(|(_, value)| value.len() as u64)
            .sum::<u64>();
        header[72..80].copy_from_slice(&value_bytes.to_le_bytes());
        header[80..88].copy_from_slice(&(payload.len() as u64).to_le_bytes());
        header[88..120].copy_from_slice(&namespace_sha256);
        header[120..152].copy_from_slice(&payload_sha256);

        let mut stream_hasher = Sha256::new();
        stream_hasher.update(STREAM_DIGEST_DOMAIN);
        stream_hasher.update(header);
        stream_hasher.update(&payload);
        let stream_sha256: [u8; 32] = stream_hasher.finalize().into();
        let mut trailer = [0u8; MIGRATION_STREAM_TRAILER_BYTES];
        trailer[..8].copy_from_slice(TRAILER_MAGIC);
        trailer[8..12].copy_from_slice(&MIGRATION_STREAM_FORMAT_VERSION.to_le_bytes());
        trailer[12..16].copy_from_slice(&(MIGRATION_STREAM_TRAILER_BYTES as u32).to_le_bytes());
        trailer[16..48].copy_from_slice(&stream_sha256);
        trailer[48..80].copy_from_slice(&namespace_sha256);

        [header.as_slice(), payload.as_slice(), trailer.as_slice()].concat()
    }

    #[test]
    fn reader_validates_complete_sorted_stream_and_eof() {
        let encoded = encode_stream(&[(key(1), b"first"), (key(2), b"")]);
        let mut reader = MigrationStreamReader::new(Cursor::new(encoded), Default::default())
            .expect("decode header");
        assert_eq!(reader.header().network_magic, 0x334f_454e);
        assert_eq!(
            reader.read_row().expect("first row").unwrap().value,
            b"first"
        );
        assert_eq!(reader.read_row().expect("second row").unwrap().value, b"");
        assert!(reader.read_row().expect("validated eof").is_none());
        let evidence = reader.finish().expect("finish stream");
        assert_eq!(evidence.header.rows, 2);
        assert_ne!(evidence.stream_sha256, [0; 32]);
    }

    #[test]
    fn reader_rejects_duplicate_or_unsorted_keys() {
        for rows in [
            vec![(key(1), b"a".as_slice()), (key(1), b"b".as_slice())],
            vec![(key(2), b"a".as_slice()), (key(1), b"b".as_slice())],
        ] {
            let encoded = encode_stream(&rows);
            let mut reader = MigrationStreamReader::new(Cursor::new(encoded), Default::default())
                .expect("decode header");
            reader.read_row().expect("first row");
            let error = reader.read_row().expect_err("invalid ordering must fail");
            assert!(error.to_string().contains("strictly increasing"));
        }
    }

    #[test]
    fn reader_rejects_bad_header_geometry_before_payload_allocation() {
        let mut encoded = encode_stream(&[(key(1), b"value")]);
        encoded[80..88].copy_from_slice(&u64::MAX.to_le_bytes());
        let error = MigrationStreamReader::new(Cursor::new(encoded), Default::default())
            .err()
            .expect("invalid geometry must fail");
        assert!(error.to_string().contains("payload length"));
    }

    #[test]
    fn reader_rejects_value_over_configured_limit_before_reading_it() {
        let encoded = encode_stream(&[(key(1), b"value")]);
        let limits = MigrationStreamLimits::new(1024, 10, 4).expect("limits");
        let mut reader =
            MigrationStreamReader::new(Cursor::new(encoded), limits).expect("decode header");
        assert!(matches!(
            reader.read_row(),
            Err(MigrationStreamError::LimitExceeded {
                limit: MigrationStreamLimit::ValueBytes,
                ..
            })
        ));
    }

    #[test]
    fn reader_rejects_payload_namespace_and_stream_digest_corruption() {
        let valid = encode_stream(&[(key(1), b"value")]);
        for offset in [
            MIGRATION_STREAM_HEADER_BYTES + PACK_KEY_BYTES + 4,
            88,
            16 + valid.len() - MIGRATION_STREAM_TRAILER_BYTES,
        ] {
            let mut encoded = valid.clone();
            encoded[offset] ^= 0x01;
            let result = (|| {
                let mut reader =
                    MigrationStreamReader::new(Cursor::new(encoded), Default::default())?;
                while reader.read_row()?.is_some() {}
                reader.finish()
            })();
            assert!(result.is_err(), "corruption at {offset} must fail");
        }
    }

    #[test]
    fn reader_requires_exact_eof_after_trailer() {
        let mut encoded = encode_stream(&[(key(1), b"value")]);
        encoded.push(0xaa);
        let mut reader = MigrationStreamReader::new(Cursor::new(encoded), Default::default())
            .expect("decode header");
        reader.read_row().expect("row");
        let error = reader.read_row().expect_err("trailing byte must fail");
        assert!(error.to_string().contains("bytes follow"));
    }

    #[test]
    fn finish_rejects_an_unconsumed_stream() {
        let encoded = encode_stream(&[(key(1), b"value")]);
        let reader = MigrationStreamReader::new(Cursor::new(encoded), Default::default())
            .expect("decode header");
        assert!(matches!(
            reader.finish(),
            Err(MigrationStreamError::Incomplete {
                read_rows: 0,
                expected_rows: 1,
            })
        ));
    }
}
