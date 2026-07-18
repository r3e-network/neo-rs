use crate::{StorageError, StorageResult};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::fs::{File, OpenOptions};
use std::io::{BufReader, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

const MAGIC: &[u8; 16] = b"NEO-PREFIX-IDX1\0";
const FORMAT_VERSION: u32 = 1;
const MAX_HEADER_BYTES: usize = 64 * 1024;
const MAX_PREFIX_BITS: u8 = 31;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
/// Exact key-domain description for a one-sided prefix occupancy bitmap.
pub struct PrefixOccupancySpec {
    /// MDBX logical table selected by the artifact (`None` is canonical).
    pub table_name: Option<String>,
    /// Fixed bytes that every eligible key must start with.
    pub key_prefix: Vec<u8>,
    /// Exact eligible key length in bytes.
    pub key_length: usize,
    /// Number of high-order key bits represented by the dense bitmap.
    pub prefix_bits: u8,
}

impl PrefixOccupancySpec {
    /// Validates and constructs a prefix occupancy key-domain description.
    pub fn new(
        table_name: Option<String>,
        key_prefix: Vec<u8>,
        key_length: usize,
        prefix_bits: u8,
    ) -> StorageResult<Self> {
        let spec = Self {
            table_name,
            key_prefix,
            key_length,
            prefix_bits,
        };
        spec.validate()?;
        Ok(spec)
    }

    fn validate(&self) -> StorageResult<()> {
        if self.key_prefix.is_empty() {
            return Err(StorageError::invalid_operation(
                "prefix occupancy key prefix must not be empty",
            ));
        }
        if !(1..=MAX_PREFIX_BITS).contains(&self.prefix_bits) {
            return Err(StorageError::invalid_operation(format!(
                "prefix occupancy bits must be in 1..={MAX_PREFIX_BITS}"
            )));
        }
        if self.key_length < self.key_prefix.len().saturating_add(4) {
            return Err(StorageError::invalid_operation(
                "prefix occupancy keys must contain four bytes after the fixed prefix",
            ));
        }
        Ok(())
    }

    fn word_count(&self) -> usize {
        (1usize << self.prefix_bits).div_ceil(u64::BITS as usize)
    }

    fn bucket(&self, key: &[u8]) -> Option<usize> {
        if key.len() != self.key_length || !key.starts_with(&self.key_prefix) {
            return None;
        }
        let start = self.key_prefix.len();
        let bytes: [u8; 4] = key.get(start..start + 4)?.try_into().ok()?;
        let prefix = u32::from_be_bytes(bytes) >> (u32::BITS as u8 - self.prefix_bits);
        Some(prefix as usize)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
/// Summary emitted after a frozen MDBX snapshot is indexed successfully.
pub struct PrefixOccupancyBuildReport {
    /// Atomically published artifact path.
    pub path: PathBuf,
    /// MDBX read transaction represented by the immutable bitmap.
    pub snapshot_transaction_id: u64,
    /// Eligible keys observed while streaming the table.
    pub indexed_keys: u64,
    /// Distinct bitmap buckets occupied by those keys.
    pub set_bits: u64,
    /// Raw bitmap payload size, excluding its checked header.
    pub bitmap_bytes: u64,
    /// Key domain bound into the artifact.
    pub spec: PrefixOccupancySpec,
}

#[derive(Debug, Serialize, Deserialize)]
struct ArtifactHeader {
    format_version: u32,
    environment_id: [u8; 16],
    snapshot_transaction_id: u64,
    indexed_keys: u64,
    set_bits: u64,
    bitmap_bytes: u64,
    bitmap_sha256: [u8; 32],
    spec: PrefixOccupancySpec,
}

pub(super) struct PrefixOccupancyBuilder {
    environment_id: [u8; 16],
    snapshot_transaction_id: u64,
    spec: PrefixOccupancySpec,
    words: Vec<u64>,
    indexed_keys: u64,
}

impl PrefixOccupancyBuilder {
    pub(super) fn new(
        environment_id: [u8; 16],
        snapshot_transaction_id: u64,
        spec: PrefixOccupancySpec,
    ) -> StorageResult<Self> {
        spec.validate()?;
        let words = vec![0; spec.word_count()];
        Ok(Self {
            environment_id,
            snapshot_transaction_id,
            spec,
            words,
            indexed_keys: 0,
        })
    }

    pub(super) fn insert(&mut self, key: &[u8]) -> bool {
        let Some(bucket) = self.spec.bucket(key) else {
            return false;
        };
        self.words[bucket / u64::BITS as usize] |= 1u64 << (bucket % u64::BITS as usize);
        self.indexed_keys = self.indexed_keys.saturating_add(1);
        true
    }

    pub(super) fn write(self, path: &Path) -> StorageResult<PrefixOccupancyBuildReport> {
        let bitmap_bytes = words_to_bytes(&self.words);
        let bitmap_sha256: [u8; 32] = Sha256::digest(&bitmap_bytes).into();
        let set_bits = self
            .words
            .iter()
            .map(|word| u64::from(word.count_ones()))
            .sum();
        let header = ArtifactHeader {
            format_version: FORMAT_VERSION,
            environment_id: self.environment_id,
            snapshot_transaction_id: self.snapshot_transaction_id,
            indexed_keys: self.indexed_keys,
            set_bits,
            bitmap_bytes: bitmap_bytes.len() as u64,
            bitmap_sha256,
            spec: self.spec.clone(),
        };
        let header_bytes = serde_json::to_vec(&header).map_err(|error| {
            StorageError::backend(format!("serialize prefix occupancy header: {error}"))
        })?;
        if header_bytes.len() > MAX_HEADER_BYTES {
            return Err(StorageError::invalid_operation(
                "prefix occupancy header exceeds the format limit",
            ));
        }

        let temporary_path = temporary_path(path);
        let file = OpenOptions::new()
            .create(true)
            .truncate(true)
            .write(true)
            .open(&temporary_path)
            .map_err(|error| {
                io_error("create prefix occupancy artifact", &temporary_path, error)
            })?;
        let mut writer = BufWriter::new(file);
        writer
            .write_all(MAGIC)
            .and_then(|()| writer.write_all(&(header_bytes.len() as u32).to_le_bytes()))
            .and_then(|()| writer.write_all(&header_bytes))
            .and_then(|()| writer.write_all(&bitmap_bytes))
            .and_then(|()| writer.flush())
            .map_err(|error| io_error("write prefix occupancy artifact", &temporary_path, error))?;
        let file = writer.into_inner().map_err(|error| {
            io_error(
                "flush prefix occupancy artifact",
                &temporary_path,
                error.into_error(),
            )
        })?;
        file.sync_all()
            .map_err(|error| io_error("sync prefix occupancy artifact", &temporary_path, error))?;
        std::fs::rename(&temporary_path, path)
            .map_err(|error| io_error("publish prefix occupancy artifact", path, error))?;
        sync_parent(path)?;

        Ok(PrefixOccupancyBuildReport {
            path: path.to_path_buf(),
            snapshot_transaction_id: self.snapshot_transaction_id,
            indexed_keys: self.indexed_keys,
            set_bits,
            bitmap_bytes: bitmap_bytes.len() as u64,
            spec: self.spec,
        })
    }
}

pub(super) struct PrefixOccupancyIndex {
    baseline_transaction_id: u64,
    covered_transaction_id: AtomicU64,
    spec: PrefixOccupancySpec,
    words: Box<[AtomicU64]>,
}

impl std::fmt::Debug for PrefixOccupancyIndex {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PrefixOccupancyIndex")
            .field("baseline_transaction_id", &self.baseline_transaction_id)
            .field(
                "covered_transaction_id",
                &self.covered_transaction_id.load(Ordering::Relaxed),
            )
            .field("spec", &self.spec)
            .finish_non_exhaustive()
    }
}

impl PrefixOccupancyIndex {
    pub(super) fn load(
        path: &Path,
        table_name: Option<&str>,
        environment_id: [u8; 16],
    ) -> StorageResult<Option<Self>> {
        let file = File::open(path)
            .map_err(|error| io_error("open prefix occupancy artifact", path, error))?;
        let mut reader = BufReader::new(file);
        let mut magic = [0u8; MAGIC.len()];
        reader
            .read_exact(&mut magic)
            .map_err(|error| io_error("read prefix occupancy magic", path, error))?;
        if &magic != MAGIC {
            return Err(StorageError::backend(
                "prefix occupancy artifact has invalid magic",
            ));
        }
        let mut header_length = [0u8; 4];
        reader
            .read_exact(&mut header_length)
            .map_err(|error| io_error("read prefix occupancy header length", path, error))?;
        let header_length = u32::from_le_bytes(header_length) as usize;
        if header_length == 0 || header_length > MAX_HEADER_BYTES {
            return Err(StorageError::backend(
                "prefix occupancy artifact has invalid header length",
            ));
        }
        let mut header_bytes = vec![0; header_length];
        reader
            .read_exact(&mut header_bytes)
            .map_err(|error| io_error("read prefix occupancy header", path, error))?;
        let header: ArtifactHeader = serde_json::from_slice(&header_bytes).map_err(|error| {
            StorageError::backend(format!("decode prefix occupancy header: {error}"))
        })?;
        header.spec.validate()?;
        if header.format_version != FORMAT_VERSION {
            return Err(StorageError::backend(format!(
                "unsupported prefix occupancy format version {}",
                header.format_version
            )));
        }
        if header.environment_id != environment_id
            || header.spec.table_name.as_deref() != table_name
        {
            return Ok(None);
        }
        let expected_bytes = header
            .spec
            .word_count()
            .saturating_mul(std::mem::size_of::<u64>());
        if header.bitmap_bytes != expected_bytes as u64 {
            return Err(StorageError::backend(
                "prefix occupancy bitmap length does not match its specification",
            ));
        }
        let mut bitmap = vec![0; expected_bytes];
        reader
            .read_exact(&mut bitmap)
            .map_err(|error| io_error("read prefix occupancy bitmap", path, error))?;
        let mut trailing = [0u8; 1];
        if reader
            .read(&mut trailing)
            .map_err(|error| io_error("validate prefix occupancy length", path, error))?
            != 0
        {
            return Err(StorageError::backend(
                "prefix occupancy artifact contains trailing bytes",
            ));
        }
        let checksum: [u8; 32] = Sha256::digest(&bitmap).into();
        if checksum != header.bitmap_sha256 {
            return Err(StorageError::backend(
                "prefix occupancy bitmap checksum mismatch",
            ));
        }
        let words = bitmap
            .chunks_exact(std::mem::size_of::<u64>())
            .map(|bytes| AtomicU64::new(u64::from_le_bytes(bytes.try_into().unwrap())))
            .collect::<Vec<_>>()
            .into_boxed_slice();
        let trust_startup = std::env::var("NEO_MDBX_PREFIX_INDEX_TRUST_STARTUP")
            .ok()
            .is_some_and(|value| value == "1" || value.eq_ignore_ascii_case("true"));
        Ok(Some(Self {
            baseline_transaction_id: header.snapshot_transaction_id,
            covered_transaction_id: AtomicU64::new(if trust_startup {
                u64::MAX
            } else {
                header.snapshot_transaction_id
            }),
            spec: header.spec,
            words,
        }))
    }

    pub(super) fn may_contain(&self, snapshot_transaction_id: u64, key: &[u8]) -> Option<bool> {
        let covered = self.covered_transaction_id.load(Ordering::Acquire);
        if snapshot_transaction_id < self.baseline_transaction_id
            || snapshot_transaction_id > covered
        {
            return None;
        }
        let bucket = self.spec.bucket(key)?;
        let word = self.words.get(bucket / u64::BITS as usize)?;
        Some(word.load(Ordering::Relaxed) & (1u64 << (bucket % u64::BITS as usize)) != 0)
    }

    pub(super) fn coverage(&self) -> (u64, u64) {
        (
            self.baseline_transaction_id,
            self.covered_transaction_id.load(Ordering::Acquire),
        )
    }

    pub(super) fn table_name(&self) -> Option<&str> {
        self.spec.table_name.as_deref()
    }

    pub(super) fn observe_put(&self, key: &[u8]) {
        let Some(bucket) = self.spec.bucket(key) else {
            return;
        };
        if let Some(word) = self.words.get(bucket / u64::BITS as usize) {
            word.fetch_or(1u64 << (bucket % u64::BITS as usize), Ordering::Relaxed);
        }
    }

    pub(super) fn advance_covered_transaction(&self, transaction_id: u64) {
        self.covered_transaction_id
            .fetch_max(transaction_id, Ordering::Release);
    }

    #[cfg(test)]
    pub(super) fn from_keys(
        environment_id: [u8; 16],
        transaction_id: u64,
        spec: PrefixOccupancySpec,
        keys: &[Vec<u8>],
    ) -> StorageResult<Self> {
        let mut builder = PrefixOccupancyBuilder::new(environment_id, transaction_id, spec)?;
        for key in keys {
            builder.insert(key);
        }
        Ok(Self {
            baseline_transaction_id: transaction_id,
            covered_transaction_id: AtomicU64::new(transaction_id),
            spec: builder.spec,
            words: builder
                .words
                .into_iter()
                .map(AtomicU64::new)
                .collect::<Vec<_>>()
                .into_boxed_slice(),
        })
    }
}

fn words_to_bytes(words: &[u64]) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(words));
    for word in words {
        bytes.extend_from_slice(&word.to_le_bytes());
    }
    bytes
}

fn temporary_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(".tmp");
    PathBuf::from(name)
}

fn sync_parent(path: &Path) -> StorageResult<()> {
    let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    else {
        return Ok(());
    };
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|error| io_error("sync prefix occupancy parent directory", parent, error))
}

fn io_error(action: &str, path: &Path, error: std::io::Error) -> StorageError {
    StorageError::Io {
        message: format!("{action} {}: {error}", path.display()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Seek, SeekFrom};
    use tempfile::TempDir;

    fn key(prefix: u8, bucket: u32, fill: u8) -> Vec<u8> {
        let mut key = vec![prefix];
        key.extend_from_slice(&bucket.to_be_bytes());
        key.resize(33, fill);
        key
    }

    fn spec() -> PrefixOccupancySpec {
        PrefixOccupancySpec::new(Some("neo_state_service".to_string()), vec![0xf0], 33, 8).unwrap()
    }

    #[test]
    fn unset_bits_prove_only_eligible_absence_at_covered_snapshots() {
        let environment_id = [7; 16];
        let present = key(0xf0, 0x1200_0000, 1);
        let colliding_absent = key(0xf0, 0x12ff_ffff, 2);
        let definite_absent = key(0xf0, 0x3400_0000, 3);
        let ineligible = key(0xef, 0x3400_0000, 4);
        let index = PrefixOccupancyIndex::from_keys(
            environment_id,
            10,
            spec(),
            std::slice::from_ref(&present),
        )
        .unwrap();

        assert_eq!(index.may_contain(10, &present), Some(true));
        assert_eq!(index.may_contain(10, &colliding_absent), Some(true));
        assert_eq!(index.may_contain(10, &definite_absent), Some(false));
        assert_eq!(index.may_contain(10, &ineligible), None);
        assert_eq!(index.may_contain(9, &definite_absent), None);
        assert_eq!(index.may_contain(11, &definite_absent), None);

        index.observe_put(&definite_absent);
        index.advance_covered_transaction(11);
        assert_eq!(index.may_contain(11, &definite_absent), Some(true));
    }

    #[test]
    fn artifact_is_bound_to_checksum_environment_table_and_transaction() {
        let temp = TempDir::new().unwrap();
        let path = temp.path().join("state.prefix");
        let environment_id = [9; 16];
        let present = key(0xf0, 0xab00_0000, 1);
        let mut builder = PrefixOccupancyBuilder::new(environment_id, 42, spec()).unwrap();
        assert!(builder.insert(&present));
        let report = builder.write(&path).unwrap();
        assert_eq!(report.indexed_keys, 1);
        assert_eq!(report.set_bits, 1);
        assert_eq!(report.bitmap_bytes, 32);

        let index = PrefixOccupancyIndex::load(&path, Some("neo_state_service"), environment_id)
            .unwrap()
            .unwrap();
        assert_eq!(index.may_contain(42, &present), Some(true));
        assert!(
            PrefixOccupancyIndex::load(&path, Some("other"), environment_id)
                .unwrap()
                .is_none()
        );
        assert!(
            PrefixOccupancyIndex::load(&path, Some("neo_state_service"), [8; 16])
                .unwrap()
                .is_none()
        );

        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .open(&path)
            .unwrap();
        file.seek(SeekFrom::End(-1)).unwrap();
        let mut byte = [0u8; 1];
        file.read_exact(&mut byte).unwrap();
        file.seek(SeekFrom::End(-1)).unwrap();
        file.write_all(&[byte[0] ^ 1]).unwrap();
        file.sync_all().unwrap();
        let error = PrefixOccupancyIndex::load(&path, Some("neo_state_service"), environment_id)
            .unwrap_err();
        assert!(error.to_string().contains("checksum mismatch"));
    }
}
