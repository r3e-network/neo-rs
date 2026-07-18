//! Persistent, fail-closed positioning for large `chain.acc` archives.
//!
//! The sidecar contains only immutable archive offsets. It is never an
//! authority for ledger data: a resumed position must still decode the first
//! imported block and match its height and previous hash against the ledger.

use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::UNIX_EPOCH;

use neo_crypto::{Crypto, Sha256Hasher};
use neo_primitives::UInt256;
use tracing::{debug, info, warn};

use super::format::{
    ChainAccFormat, ChainAccHeader, read_next_chain_acc_block, skip_chain_acc_records,
    skip_chain_acc_records_observed,
};

const INDEX_MAGIC: [u8; 8] = *b"N3CAIDX\0";
const INDEX_VERSION: u32 = 1;
const CHECKPOINT_STRIDE: u64 = 4_096;
const ARCHIVE_EDGE_SAMPLE_BYTES: usize = 4 * 1_024;
const INDEX_CHECKSUM_BYTES: usize = 32;
const MAX_INDEX_FILE_BYTES: u64 = 32 * 1_024 * 1_024;

static TEMP_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[derive(Debug, Clone, PartialEq, Eq)]
struct ArchiveIdentity {
    format: ChainAccFormat,
    count: u64,
    start_height: Option<u32>,
    data_offset: u64,
    file_len: u64,
    modified_secs: u64,
    modified_nanos: u32,
    device: u64,
    inode: u64,
    edge_sha256: [u8; 32],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct OffsetCheckpoint {
    record: u64,
    offset: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChainAccOffsetIndex {
    identity: ArchiveIdentity,
    checkpoints: Vec<OffsetCheckpoint>,
    frontier: OffsetCheckpoint,
}

/// A bounded index recorder attached to one active archive reader.
pub(super) struct ChainAccOffsetIndexSession {
    archive_path: PathBuf,
    index_path: PathBuf,
    index: ChainAccOffsetIndex,
    reader_offset: u64,
    dirty: bool,
    enabled: bool,
}

pub(super) struct ChainAccReaderPosition {
    pub(super) offset_index: Option<ChainAccOffsetIndexSession>,
    pub(super) offset: u64,
    pub(super) index_hit: bool,
    pub(super) index_rebuilt: bool,
}

#[derive(Debug, Clone, Copy)]
pub(super) struct ChainAccResumeValidation<'a> {
    pub(super) has_first_record: bool,
    pub(super) expected_height: Option<u32>,
    pub(super) expected_prev_hash: Option<&'a UInt256>,
}

/// Positions `reader` at `records_to_skip`, returning an optional recorder.
///
/// Sidecar failures never fail the import. A loaded offset is used only after
/// the first resumed block matches both the expected height and ledger link.
pub(super) fn position_chain_acc_reader<R>(
    reader: &mut R,
    archive_path: Option<&Path>,
    header: ChainAccHeader,
    records_to_skip: usize,
    validation: ChainAccResumeValidation<'_>,
) -> anyhow::Result<ChainAccReaderPosition>
where
    R: BufRead + Seek,
{
    let target_record = u64::try_from(records_to_skip)
        .map_err(|_| anyhow::anyhow!("chain.acc skip count does not fit u64"))?;
    if target_record == 0 && !validation.has_first_record {
        return Ok(ChainAccReaderPosition {
            offset_index: None,
            offset: reader.stream_position()?,
            index_hit: false,
            index_rebuilt: false,
        });
    }
    let Some(archive_path) = archive_path else {
        skip_chain_acc_records(reader, records_to_skip)?;
        return Ok(ChainAccReaderPosition {
            offset_index: None,
            offset: reader.stream_position()?,
            index_hit: false,
            index_rebuilt: false,
        });
    };

    let identity = match ArchiveIdentity::read(archive_path, header) {
        Ok(identity) => identity,
        Err(error) => {
            debug!(
                target: "neo::import",
                file = %archive_path.display(),
                error = %error,
                "chain.acc offset index unavailable; using sequential positioning"
            );
            reader.seek(SeekFrom::Start(header.data_offset))?;
            skip_chain_acc_records(reader, records_to_skip)?;
            return Ok(ChainAccReaderPosition {
                offset_index: None,
                offset: reader.stream_position()?,
                index_hit: false,
                index_rebuilt: false,
            });
        }
    };

    if target_record > identity.count {
        anyhow::bail!(
            "chain.acc skip count {target_record} exceeds archive count {}",
            identity.count
        );
    }

    let index_path = chain_acc_index_path(archive_path);
    let mut loaded_index = false;
    let mut rejected_index = false;
    let mut index = match load_index(&index_path, &identity) {
        Ok(Some(index)) => {
            loaded_index = true;
            index
        }
        Ok(None) => ChainAccOffsetIndex::new(identity.clone()),
        Err(error) => {
            rejected_index = true;
            warn!(
                target: "neo::import",
                index = %index_path.display(),
                error = %error,
                "ignoring invalid chain.acc offset index"
            );
            ChainAccOffsetIndex::new(identity.clone())
        }
    };

    let checkpoint = index.best_checkpoint(target_record);
    let trusted_nonzero_offset = loaded_index && checkpoint.record > 0;
    let original_index = index.clone();
    let positioned = scan_from_checkpoint(reader, &mut index, checkpoint, target_record);
    let positioned_offset = match positioned {
        Ok(offset) => offset,
        Err(error) if trusted_nonzero_offset => {
            warn!(
                target: "neo::import",
                index = %index_path.display(),
                error = %error,
                "chain.acc offset index seek failed; rebuilding sequentially"
            );
            rejected_index = true;
            index = ChainAccOffsetIndex::new(identity.clone());
            sequential_position(reader, &mut index, target_record)?
        }
        Err(error) => return Err(error),
    };

    let trusted_position_is_valid = !trusted_nonzero_offset
        || validate_resumed_position(reader, positioned_offset, target_record, validation)?;
    let positioned_offset = if trusted_position_is_valid {
        positioned_offset
    } else {
        warn!(
            target: "neo::import",
            index = %index_path.display(),
            record = target_record,
            offset = positioned_offset,
            "chain.acc offset index failed resume validation; rebuilding sequentially"
        );
        rejected_index = true;
        index = ChainAccOffsetIndex::new(identity.clone());
        sequential_position(reader, &mut index, target_record)?
    };

    let mut session = ChainAccOffsetIndexSession {
        archive_path: archive_path.to_path_buf(),
        index_path,
        reader_offset: positioned_offset,
        dirty: index != original_index || (!loaded_index && target_record > 0),
        index,
        enabled: true,
    };
    session.persist_best_effort();

    let index_hit = trusted_nonzero_offset && !rejected_index;
    let index_rebuilt = rejected_index || !loaded_index;
    info!(
        target: "neo::import",
        file = %archive_path.display(),
        record = target_record,
        offset = positioned_offset,
        index_hit,
        index_rebuilt,
        "positioned chain.acc reader"
    );
    Ok(ChainAccReaderPosition {
        offset_index: Some(session),
        offset: positioned_offset,
        index_hit,
        index_rebuilt,
    })
}

impl ChainAccOffsetIndexSession {
    /// Records the byte offset immediately after one validated archive record.
    pub(super) fn observe_record(&mut self, next_record: usize, payload_len: usize) {
        if !self.enabled {
            return;
        }
        let result = (|| {
            let payload_len = u64::try_from(payload_len)
                .map_err(|_| anyhow::anyhow!("chain.acc payload length does not fit u64"))?;
            let next_offset = self
                .reader_offset
                .checked_add(4)
                .and_then(|offset| offset.checked_add(payload_len))
                .ok_or_else(|| anyhow::anyhow!("chain.acc reader offset overflow"))?;
            let next_record = u64::try_from(next_record)
                .map_err(|_| anyhow::anyhow!("chain.acc record does not fit u64"))?;
            let advanced = self.index.observe(next_record, next_offset)?;
            self.reader_offset = next_offset;
            self.dirty |= advanced;
            anyhow::Ok(())
        })();
        if let Err(error) = result {
            warn!(
                target: "neo::import",
                index = %self.index_path.display(),
                error = %error,
                "disabling chain.acc offset index recorder"
            );
            self.enabled = false;
        }
    }

    /// Atomically publishes newly observed checkpoints without affecting import.
    pub(super) fn persist_best_effort(&mut self) {
        if !self.enabled || !self.dirty {
            return;
        }
        let result = (|| {
            let current_identity = ArchiveIdentity::read(
                &self.archive_path,
                ChainAccHeader {
                    count: usize::try_from(self.index.identity.count)
                        .map_err(|_| anyhow::anyhow!("chain.acc index count does not fit usize"))?,
                    start_height: self.index.identity.start_height,
                    data_offset: self.index.identity.data_offset,
                    format: self.index.identity.format,
                },
            )?;
            if current_identity != self.index.identity {
                anyhow::bail!("chain.acc archive identity changed while importing");
            }
            write_index_atomically(&self.index_path, &self.index)
        })();
        match result {
            Ok(()) => self.dirty = false,
            Err(error) => warn!(
                target: "neo::import",
                index = %self.index_path.display(),
                error = %error,
                "failed to persist chain.acc offset index; import remains unaffected"
            ),
        }
    }
}

impl ArchiveIdentity {
    fn read(path: &Path, header: ChainAccHeader) -> anyhow::Result<Self> {
        let mut file = File::open(path)
            .map_err(|error| anyhow::anyhow!("opening {}: {error}", path.display()))?;
        let metadata = file
            .metadata()
            .map_err(|error| anyhow::anyhow!("reading {} metadata: {error}", path.display()))?;
        let file_len = metadata.len();
        if header.data_offset > file_len {
            anyhow::bail!(
                "chain.acc data offset {} exceeds file length {file_len}",
                header.data_offset
            );
        }
        let modified = metadata
            .modified()
            .map_err(|error| anyhow::anyhow!("reading {} mtime: {error}", path.display()))?
            .duration_since(UNIX_EPOCH)
            .map_err(|_| anyhow::anyhow!("{} mtime predates Unix epoch", path.display()))?;

        let edge_sha256 = archive_edge_sha256(&mut file, file_len)?;
        let after = file
            .metadata()
            .map_err(|error| anyhow::anyhow!("re-reading {} metadata: {error}", path.display()))?;
        if after.len() != file_len || after.modified().ok() != metadata.modified().ok() {
            anyhow::bail!("chain.acc archive changed while computing its identity");
        }

        #[cfg(unix)]
        let (device, inode) = {
            use std::os::unix::fs::MetadataExt;
            (metadata.dev(), metadata.ino())
        };
        #[cfg(not(unix))]
        let (device, inode) = (0, 0);

        Ok(Self {
            format: header.format,
            count: u64::try_from(header.count)
                .map_err(|_| anyhow::anyhow!("chain.acc count does not fit u64"))?,
            start_height: header.start_height,
            data_offset: header.data_offset,
            file_len,
            modified_secs: modified.as_secs(),
            modified_nanos: modified.subsec_nanos(),
            device,
            inode,
            edge_sha256,
        })
    }
}

impl ChainAccOffsetIndex {
    fn new(identity: ArchiveIdentity) -> Self {
        let origin = OffsetCheckpoint {
            record: 0,
            offset: identity.data_offset,
        };
        Self {
            identity,
            checkpoints: vec![origin],
            frontier: origin,
        }
    }

    fn best_checkpoint(&self, target_record: u64) -> OffsetCheckpoint {
        let checkpoint_index = usize::try_from(target_record / CHECKPOINT_STRIDE)
            .unwrap_or(usize::MAX)
            .min(self.checkpoints.len().saturating_sub(1));
        let checkpoint = self.checkpoints[checkpoint_index];
        if self.frontier.record <= target_record && self.frontier.record >= checkpoint.record {
            self.frontier
        } else {
            checkpoint
        }
    }

    fn observe(&mut self, next_record: u64, next_offset: u64) -> anyhow::Result<bool> {
        if next_record > self.identity.count {
            anyhow::bail!(
                "chain.acc index record {next_record} exceeds count {}",
                self.identity.count
            );
        }
        if next_offset > self.identity.file_len {
            anyhow::bail!(
                "chain.acc index offset {next_offset} exceeds file length {}",
                self.identity.file_len
            );
        }
        if next_record < self.frontier.record {
            if next_record.is_multiple_of(CHECKPOINT_STRIDE) {
                let checkpoint_index = usize::try_from(next_record / CHECKPOINT_STRIDE)
                    .map_err(|_| anyhow::anyhow!("chain.acc checkpoint index overflow"))?;
                let expected = self.checkpoints.get(checkpoint_index).ok_or_else(|| {
                    anyhow::anyhow!("chain.acc index is missing an observed checkpoint")
                })?;
                if next_offset != expected.offset {
                    anyhow::bail!(
                        "chain.acc checkpoint offset mismatch: expected {}, got {next_offset}",
                        expected.offset
                    );
                }
            }
            return Ok(false);
        }
        if next_record == self.frontier.record {
            if next_offset != self.frontier.offset {
                anyhow::bail!(
                    "chain.acc indexed frontier offset mismatch: expected {}, got {next_offset}",
                    self.frontier.offset
                );
            }
            return Ok(false);
        }
        if next_record != self.frontier.record + 1 {
            anyhow::bail!(
                "chain.acc index observation gap: frontier {}, next {next_record}",
                self.frontier.record
            );
        }
        if next_offset <= self.frontier.offset {
            anyhow::bail!(
                "chain.acc index offsets are not increasing: {} then {next_offset}",
                self.frontier.offset
            );
        }
        self.frontier = OffsetCheckpoint {
            record: next_record,
            offset: next_offset,
        };
        if next_record.is_multiple_of(CHECKPOINT_STRIDE) {
            self.checkpoints.push(self.frontier);
        }
        Ok(true)
    }

    fn validate(&self) -> anyhow::Result<()> {
        if self.checkpoints.is_empty() {
            anyhow::bail!("chain.acc index has no origin checkpoint");
        }
        let max_entries = self.identity.count / CHECKPOINT_STRIDE + 1;
        if u64::try_from(self.checkpoints.len()).unwrap_or(u64::MAX) > max_entries {
            anyhow::bail!("chain.acc index exceeds its checkpoint bound");
        }
        for (position, checkpoint) in self.checkpoints.iter().enumerate() {
            let expected_record = u64::try_from(position)
                .map_err(|_| anyhow::anyhow!("chain.acc checkpoint position overflow"))?
                .checked_mul(CHECKPOINT_STRIDE)
                .ok_or_else(|| anyhow::anyhow!("chain.acc checkpoint record overflow"))?;
            if checkpoint.record != expected_record {
                anyhow::bail!(
                    "chain.acc checkpoint {position} has record {}, expected {expected_record}",
                    checkpoint.record
                );
            }
            if checkpoint.record > self.frontier.record {
                anyhow::bail!("chain.acc checkpoint exceeds indexed frontier");
            }
            if position == 0 {
                if checkpoint.offset != self.identity.data_offset {
                    anyhow::bail!("chain.acc index origin does not match archive data offset");
                }
            } else if checkpoint.offset <= self.checkpoints[position - 1].offset {
                anyhow::bail!("chain.acc checkpoint offsets are not strictly increasing");
            }
        }
        if self.frontier.record > self.identity.count
            || self.frontier.offset > self.identity.file_len
        {
            anyhow::bail!("chain.acc index frontier exceeds archive bounds");
        }
        let last = self.checkpoints[self.checkpoints.len() - 1];
        if last.record != (self.frontier.record / CHECKPOINT_STRIDE) * CHECKPOINT_STRIDE {
            anyhow::bail!("chain.acc index is missing a fixed checkpoint");
        }
        if self.frontier.record == last.record {
            if self.frontier.offset != last.offset {
                anyhow::bail!("chain.acc frontier disagrees with its fixed checkpoint");
            }
        } else if self.frontier.offset <= last.offset {
            anyhow::bail!("chain.acc frontier offset is not after its checkpoint");
        }
        Ok(())
    }
}

fn scan_from_checkpoint<R>(
    reader: &mut R,
    index: &mut ChainAccOffsetIndex,
    checkpoint: OffsetCheckpoint,
    target_record: u64,
) -> anyhow::Result<u64>
where
    R: BufRead + Seek,
{
    reader.seek(SeekFrom::Start(checkpoint.offset))?;
    let records = target_record
        .checked_sub(checkpoint.record)
        .ok_or_else(|| anyhow::anyhow!("chain.acc checkpoint is after target record"))?;
    let records = usize::try_from(records)
        .map_err(|_| anyhow::anyhow!("chain.acc residual scan does not fit usize"))?;
    let first_record = usize::try_from(checkpoint.record)
        .map_err(|_| anyhow::anyhow!("chain.acc checkpoint record does not fit usize"))?;
    scan_records(
        reader,
        first_record,
        records,
        checkpoint.offset,
        |next_record, next_offset| {
            index.observe(next_record, next_offset)?;
            Ok(())
        },
    )
}

fn sequential_position<R>(
    reader: &mut R,
    index: &mut ChainAccOffsetIndex,
    target_record: u64,
) -> anyhow::Result<u64>
where
    R: BufRead + Seek,
{
    reader.seek(SeekFrom::Start(index.identity.data_offset))?;
    let records = usize::try_from(target_record)
        .map_err(|_| anyhow::anyhow!("chain.acc sequential scan does not fit usize"))?;
    scan_records(
        reader,
        0,
        records,
        index.identity.data_offset,
        |next_record, next_offset| {
            index.observe(next_record, next_offset)?;
            Ok(())
        },
    )
}

fn scan_records<R, F>(
    reader: &mut R,
    first_record: usize,
    records: usize,
    offset: u64,
    mut observe: F,
) -> anyhow::Result<u64>
where
    R: BufRead + Seek,
    F: FnMut(u64, u64) -> anyhow::Result<()>,
{
    skip_chain_acc_records_observed(reader, first_record, records, offset, |record, offset| {
        observe(record, offset)
    })
}

fn validate_resumed_position<R>(
    reader: &mut R,
    offset: u64,
    target_record: u64,
    validation: ChainAccResumeValidation<'_>,
) -> anyhow::Result<bool>
where
    R: BufRead + Seek,
{
    if !validation.has_first_record {
        return Ok(false);
    }
    let (Some(expected_height), Some(expected_prev_hash)) =
        (validation.expected_height, validation.expected_prev_hash)
    else {
        return Ok(false);
    };

    reader.seek(SeekFrom::Start(offset))?;
    let mut block_bytes = Vec::new();
    let result = usize::try_from(target_record)
        .map_err(|_| anyhow::anyhow!("chain.acc target record does not fit usize"))
        .and_then(|record| read_next_chain_acc_block(reader, record, &mut block_bytes));
    reader.seek(SeekFrom::Start(offset))?;
    let Ok(block) = result else {
        return Ok(false);
    };
    Ok(block.index() == expected_height && block.prev_hash() == expected_prev_hash)
}

fn archive_edge_sha256(file: &mut File, file_len: u64) -> anyhow::Result<[u8; 32]> {
    let sample_len = usize::try_from(file_len.min(ARCHIVE_EDGE_SAMPLE_BYTES as u64))
        .map_err(|_| anyhow::anyhow!("chain.acc edge sample length does not fit usize"))?;
    let mut first = vec![0u8; sample_len];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut first)?;

    let last_offset = file_len.saturating_sub(sample_len as u64);
    let mut last = vec![0u8; sample_len];
    file.seek(SeekFrom::Start(last_offset))?;
    file.read_exact(&mut last)?;

    let mut hasher = Sha256Hasher::new();
    hasher.update(b"neo-chain-acc-edge-v1");
    hasher.update(&file_len.to_le_bytes());
    hasher.update(&first);
    hasher.update(&last);
    Ok(hasher.finalize())
}

fn chain_acc_index_path(archive_path: &Path) -> PathBuf {
    let mut path: OsString = archive_path.as_os_str().to_owned();
    path.push(".idx");
    PathBuf::from(path)
}

fn load_index(
    path: &Path,
    expected_identity: &ArchiveIdentity,
) -> anyhow::Result<Option<ChainAccOffsetIndex>> {
    let file = match File::open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(anyhow::anyhow!("opening {}: {error}", path.display())),
    };
    let metadata = file
        .metadata()
        .map_err(|error| anyhow::anyhow!("reading {} metadata: {error}", path.display()))?;
    if metadata.len() > MAX_INDEX_FILE_BYTES {
        anyhow::bail!(
            "chain.acc index is {} bytes, above the {}-byte bound",
            metadata.len(),
            MAX_INDEX_FILE_BYTES
        );
    }
    let capacity = usize::try_from(metadata.len())
        .unwrap_or(0)
        .min(MAX_INDEX_FILE_BYTES as usize);
    let mut bytes = Vec::with_capacity(capacity);
    file.take(MAX_INDEX_FILE_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|error| anyhow::anyhow!("reading {}: {error}", path.display()))?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_INDEX_FILE_BYTES {
        anyhow::bail!("chain.acc index grew above its file-size bound while being read");
    }
    let index = decode_index(&bytes)?;
    if &index.identity != expected_identity {
        anyhow::bail!("chain.acc index archive identity mismatch");
    }
    index.validate()?;
    Ok(Some(index))
}

fn write_index_atomically(path: &Path, index: &ChainAccOffsetIndex) -> anyhow::Result<()> {
    index.validate()?;
    let bytes = encode_index(index)?;
    if u64::try_from(bytes.len()).unwrap_or(u64::MAX) > MAX_INDEX_FILE_BYTES {
        anyhow::bail!("encoded chain.acc index exceeds its file-size bound");
    }
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let sequence = TEMP_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("chain.acc.idx");
    let temporary = parent.join(format!(
        ".{file_name}.tmp.{}.{}",
        std::process::id(),
        sequence
    ));

    let result = (|| {
        let mut file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&temporary, path)?;
        File::open(parent)?.sync_all()?;
        anyhow::Ok(())
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn encode_index(index: &ChainAccOffsetIndex) -> anyhow::Result<Vec<u8>> {
    let entry_count = u32::try_from(index.checkpoints.len())
        .map_err(|_| anyhow::anyhow!("chain.acc index has too many checkpoints"))?;
    let mut bytes = Vec::with_capacity(160 + index.checkpoints.len() * 16);
    bytes.extend_from_slice(&INDEX_MAGIC);
    bytes.extend_from_slice(&INDEX_VERSION.to_le_bytes());
    bytes.extend_from_slice(&CHECKPOINT_STRIDE.to_le_bytes());
    bytes.push(index.identity.format as u8);
    bytes.push(u8::from(index.identity.start_height.is_some()));
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&index.identity.count.to_le_bytes());
    bytes.extend_from_slice(
        &index
            .identity
            .start_height
            .unwrap_or_default()
            .to_le_bytes(),
    );
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&index.identity.data_offset.to_le_bytes());
    bytes.extend_from_slice(&index.identity.file_len.to_le_bytes());
    bytes.extend_from_slice(&index.identity.modified_secs.to_le_bytes());
    bytes.extend_from_slice(&index.identity.modified_nanos.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.extend_from_slice(&index.identity.device.to_le_bytes());
    bytes.extend_from_slice(&index.identity.inode.to_le_bytes());
    bytes.extend_from_slice(&index.identity.edge_sha256);
    bytes.extend_from_slice(&index.frontier.record.to_le_bytes());
    bytes.extend_from_slice(&index.frontier.offset.to_le_bytes());
    bytes.extend_from_slice(&entry_count.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    for checkpoint in &index.checkpoints {
        bytes.extend_from_slice(&checkpoint.record.to_le_bytes());
        bytes.extend_from_slice(&checkpoint.offset.to_le_bytes());
    }
    let checksum = Crypto::sha256(&bytes);
    bytes.extend_from_slice(&checksum);
    Ok(bytes)
}

fn decode_index(bytes: &[u8]) -> anyhow::Result<ChainAccOffsetIndex> {
    if bytes.len() < INDEX_CHECKSUM_BYTES {
        anyhow::bail!("chain.acc index is truncated");
    }
    let content_len = bytes.len() - INDEX_CHECKSUM_BYTES;
    let (content, checksum) = bytes.split_at(content_len);
    if Crypto::sha256(content).as_slice() != checksum {
        anyhow::bail!("chain.acc index checksum mismatch");
    }
    let mut cursor = ByteCursor::new(content);
    if cursor.array::<8>()? != INDEX_MAGIC {
        anyhow::bail!("chain.acc index magic mismatch");
    }
    if cursor.u32()? != INDEX_VERSION {
        anyhow::bail!("unsupported chain.acc index version");
    }
    if cursor.u64()? != CHECKPOINT_STRIDE {
        anyhow::bail!("chain.acc index checkpoint stride mismatch");
    }
    let format = match cursor.u8()? {
        value if value == ChainAccFormat::CountOnly as u8 => ChainAccFormat::CountOnly,
        value if value == ChainAccFormat::HeightPrefixed as u8 => ChainAccFormat::HeightPrefixed,
        _ => anyhow::bail!("chain.acc index format tag is invalid"),
    };
    let has_start_height = match cursor.u8()? {
        0 => false,
        1 => true,
        _ => anyhow::bail!("chain.acc index start-height tag is invalid"),
    };
    cursor.zeroes(6)?;
    let count = cursor.u64()?;
    let encoded_start_height = cursor.u32()?;
    cursor.zeroes(4)?;
    let identity = ArchiveIdentity {
        format,
        count,
        start_height: has_start_height.then_some(encoded_start_height),
        data_offset: cursor.u64()?,
        file_len: cursor.u64()?,
        modified_secs: cursor.u64()?,
        modified_nanos: cursor.u32()?,
        device: {
            cursor.zeroes(4)?;
            cursor.u64()?
        },
        inode: cursor.u64()?,
        edge_sha256: cursor.array::<32>()?,
    };
    let frontier = OffsetCheckpoint {
        record: cursor.u64()?,
        offset: cursor.u64()?,
    };
    let entry_count = cursor.u32()?;
    cursor.zeroes(4)?;
    let max_entries = count / CHECKPOINT_STRIDE + 1;
    if u64::from(entry_count) > max_entries {
        anyhow::bail!("chain.acc index entry count exceeds its archive bound");
    }
    let remaining_entries_bytes = usize::try_from(entry_count)
        .ok()
        .and_then(|count| count.checked_mul(16))
        .ok_or_else(|| anyhow::anyhow!("chain.acc index entry byte count overflow"))?;
    if cursor.remaining() != remaining_entries_bytes {
        anyhow::bail!("chain.acc index length does not match its entry count");
    }
    let mut checkpoints = Vec::with_capacity(entry_count as usize);
    for _ in 0..entry_count {
        checkpoints.push(OffsetCheckpoint {
            record: cursor.u64()?,
            offset: cursor.u64()?,
        });
    }
    let index = ChainAccOffsetIndex {
        identity,
        checkpoints,
        frontier,
    };
    index.validate()?;
    Ok(index)
}

struct ByteCursor<'a> {
    bytes: &'a [u8],
    position: usize,
}

impl<'a> ByteCursor<'a> {
    fn new(bytes: &'a [u8]) -> Self {
        Self { bytes, position: 0 }
    }

    fn remaining(&self) -> usize {
        self.bytes.len().saturating_sub(self.position)
    }

    fn take(&mut self, len: usize) -> anyhow::Result<&'a [u8]> {
        let end = self
            .position
            .checked_add(len)
            .ok_or_else(|| anyhow::anyhow!("chain.acc index cursor overflow"))?;
        let value = self
            .bytes
            .get(self.position..end)
            .ok_or_else(|| anyhow::anyhow!("chain.acc index is truncated"))?;
        self.position = end;
        Ok(value)
    }

    fn array<const N: usize>(&mut self) -> anyhow::Result<[u8; N]> {
        self.take(N)?
            .try_into()
            .map_err(|_| anyhow::anyhow!("chain.acc index field has the wrong length"))
    }

    fn u8(&mut self) -> anyhow::Result<u8> {
        Ok(self.array::<1>()?[0])
    }

    fn u32(&mut self) -> anyhow::Result<u32> {
        Ok(u32::from_le_bytes(self.array()?))
    }

    fn u64(&mut self) -> anyhow::Result<u64> {
        Ok(u64::from_le_bytes(self.array()?))
    }

    fn zeroes(&mut self, len: usize) -> anyhow::Result<()> {
        if self.take(len)?.iter().any(|byte| *byte != 0) {
            anyhow::bail!("chain.acc index reserved bytes are non-zero");
        }
        Ok(())
    }
}
