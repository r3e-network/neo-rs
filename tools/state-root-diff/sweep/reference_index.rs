//! Disk-backed `height -> root hash` index for the reference stream.
//!
//! The production reference file is 1.36 GB / 13.15M rows and its record order
//! is *not* guaranteed to be ascending, so a streaming compare against the
//! local iterator cannot assume matched order. Loading it into memory is not an
//! option either: 13.15M hashes are 420 MB before hash-map overhead.
//!
//! This index keeps memory at two fixed buffers and uses a temporary file as a
//! height-addressed slot array:
//!
//! ```text
//! slot offset = height * SLOT_BYTES
//! slot layout = [present: 1 byte][root hash: 32 bytes]
//! ```
//!
//! Disk use is `33 * (max height indexed + 1)` bytes (~434 MB for heights up to
//! 13_150_240). Writes are coalesced into a sequential batch whenever heights
//! arrive in ascending order (the fast path for a sorted reference); unordered
//! input falls back to one `seek` + `write` per slot, still in bounded memory.
//!
//! Duplicate heights are detected exactly: every non-contiguous insert reads
//! the slot back (unbuffered) before overwriting, so a reference that states two
//! different hashes for one height is reported instead of silently resolved by
//! last-write-wins.

use std::fs::{File, OpenOptions};
use std::io::{self, BufReader, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Length of a state root hash.
pub const HASH_LEN: usize = 32;
/// Bytes per height slot: one presence flag plus one hash.
pub const SLOT_BYTES: u64 = 33;
/// Presence flag value marking an occupied slot.
const PRESENT: u8 = 1;

/// Slots buffered before a sequential batch is written (8192 * 33 = 264 KiB).
const WRITE_BATCH_SLOTS: usize = 8192;
/// Read buffer used for forward lookups during the comparison pass.
const READ_BUFFER_BYTES: usize = 1 << 20;

/// Aggregate facts about a filled index, reported in the JSON result.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IndexStats {
    /// Distinct heights stored.
    pub heights: u64,
    /// Highest stored height.
    pub max_height: Option<u32>,
    /// Repeats of the same height carrying an identical hash.
    pub duplicate_identical: u64,
    /// Repeats of the same height carrying a different hash (reference corruption).
    pub duplicate_conflicting: u64,
    /// Lowest height whose duplicate conflicts.
    pub first_conflict_height: Option<u32>,
}

/// Height-addressed hash slots backed by a caller-owned temporary file.
pub struct HeightHashIndex {
    path: PathBuf,
    keep: bool,
    writer: BufWriter<File>,
    readback: File,
    reader: Option<BufReader<File>>,
    /// Logical byte offset of `reader`, tracked by hand because `BufReader`
    /// hides its buffered position. Lookups during a sweep are ascending, so
    /// this turns every slot read into a buffered sequential read.
    read_position: u64,
    batch_start: Option<u32>,
    batch: Vec<u8>,
    stats: IndexStats,
}

impl HeightHashIndex {
    /// Creates an empty index in `dir`.
    ///
    /// The file is deleted on drop unless `keep` is set.
    pub fn create(dir: &Path, keep: bool) -> io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let path = dir.join(format!("state-root-ref-index-{}.bin", std::process::id()));
        let writer_file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .open(&path)?;
        let readback = OpenOptions::new().read(true).open(&path)?;
        Ok(Self {
            path,
            keep,
            writer: BufWriter::with_capacity(WRITE_BATCH_SLOTS * SLOT_BYTES as usize, writer_file),
            readback,
            reader: None,
            read_position: 0,
            batch_start: None,
            batch: Vec::with_capacity(WRITE_BATCH_SLOTS * SLOT_BYTES as usize),
            stats: IndexStats::default(),
        })
    }

    /// Path of the temporary slot file.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Bytes reserved by the slot file (worst-case temporary disk use).
    #[must_use]
    pub fn allocated_bytes(&self) -> u64 {
        self.stats
            .max_height
            .map_or(0, |h| (u64::from(h) + 1) * SLOT_BYTES)
    }

    /// Statistics gathered while filling the index.
    #[must_use]
    pub fn stats(&self) -> &IndexStats {
        &self.stats
    }

    /// Stores `hash` for `height`, coalescing ascending input into batches.
    ///
    /// A repeat of an already stored height overwrites the slot but does not
    /// count as a distinct height, so `heights` stays a slot-occupancy count.
    pub fn insert(&mut self, height: u32, hash: &[u8; HASH_LEN]) -> io::Result<()> {
        // The sequential fast path is only safe for a height above everything
        // stored so far: such a slot cannot have been written yet, so there is
        // nothing to read back. A height at or below the high-water mark might
        // be a repeat even when it happens to continue the pending batch (the
        // reference can emit `5, 4, 5`), so those take the readback path.
        let known = self
            .stats
            .max_height
            .is_some_and(|highest| height <= highest);
        let next_slot = self
            .batch_start
            .is_some_and(|start| u64::from(height) == u64::from(start) + self.batch_slots() as u64);
        let repeat = if next_slot && !known {
            false
        } else {
            self.flush_batch()?;
            let repeat = self.check_duplicate(height, hash)?;
            self.batch_start = Some(height);
            repeat
        };
        self.batch.push(PRESENT);
        self.batch.extend_from_slice(hash);
        if self.batch_slots() >= WRITE_BATCH_SLOTS {
            self.flush_batch()?;
        }
        if !repeat {
            self.stats.heights += 1;
            self.stats.max_height = Some(self.stats.max_height.map_or(height, |h| h.max(height)));
        }
        Ok(())
    }

    /// Flushes pending writes and opens the buffered forward read path.
    ///
    /// Lookups must only happen after sealing: the read cache would otherwise
    /// be able to return a slot value that a later insert has already replaced.
    pub fn seal(&mut self) -> io::Result<()> {
        self.flush_batch()?;
        self.writer.flush()?;
        if self.reader.is_none() {
            let file = OpenOptions::new().read(true).open(&self.path)?;
            self.reader = Some(BufReader::with_capacity(READ_BUFFER_BYTES, file));
            self.read_position = 0;
        }
        Ok(())
    }

    /// Returns the stored hash for `height` (`None` when absent or out of range).
    pub fn get(&mut self, height: u32) -> io::Result<Option<[u8; HASH_LEN]>> {
        let Some(height) = self.clamp_to_range(height) else {
            return Ok(None);
        };
        let offset = u64::from(height) * SLOT_BYTES;
        let reader = self
            .reader
            .as_mut()
            .ok_or_else(|| io::Error::other("height index must be sealed before lookups begin"))?;
        if offset != self.read_position {
            if offset > self.read_position {
                // Forward skip: stays inside the read buffer when it can.
                reader.seek_relative((offset - self.read_position) as i64)?;
            } else {
                reader.seek(SeekFrom::Start(offset))?;
            }
            self.read_position = offset;
        }
        let mut slot = [0u8; SLOT_BYTES as usize];
        match reader.read_exact(&mut slot) {
            Ok(()) => self.read_position += SLOT_BYTES,
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(None),
            Err(error) => return Err(error),
        }
        if slot[0] != PRESENT {
            return Ok(None);
        }
        let mut hash = [0u8; HASH_LEN];
        hash.copy_from_slice(&slot[1..]);
        Ok(Some(hash))
    }

    fn batch_slots(&self) -> usize {
        self.batch.len() / SLOT_BYTES as usize
    }

    fn clamp_to_range(&self, height: u32) -> Option<u32> {
        match self.stats.max_height {
            Some(max) if height <= max => Some(height),
            _ => None,
        }
    }

    fn flush_batch(&mut self) -> io::Result<()> {
        let Some(start) = self.batch_start.take() else {
            return Ok(());
        };
        if self.batch.is_empty() {
            return Ok(());
        }
        self.writer
            .seek(SeekFrom::Start(u64::from(start) * SLOT_BYTES))?;
        self.writer.write_all(&self.batch)?;
        self.batch.clear();
        Ok(())
    }

    /// Reads the slot for `height` back through an unbuffered handle so the
    /// duplicate check never observes a stale cached block, and classifies the
    /// repeat as identical or conflicting against the incoming hash.
    ///
    /// Returns `true` when the slot already held a hash (i.e. the height was
    /// seen before). The pending batch is flushed to the OS first: the readback
    /// handle is a separate file descriptor and cannot see bytes that are still
    /// buffered in the writer.
    fn check_duplicate(&mut self, height: u32, hash: &[u8; HASH_LEN]) -> io::Result<bool> {
        if self.stats.max_height.is_none_or(|highest| height > highest) {
            return Ok(false);
        }
        self.writer.flush()?;
        self.readback
            .seek(SeekFrom::Start(u64::from(height) * SLOT_BYTES))?;
        let mut slot = [0u8; SLOT_BYTES as usize];
        match self.readback.read_exact(&mut slot) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::UnexpectedEof => return Ok(false),
            Err(error) => return Err(error),
        }
        if slot[0] != PRESENT {
            return Ok(false);
        }
        if slot[1..] == hash[..] {
            self.stats.duplicate_identical += 1;
        } else {
            self.stats.duplicate_conflicting += 1;
            self.stats.first_conflict_height.get_or_insert(height);
        }
        Ok(true)
    }
}

impl Drop for HeightHashIndex {
    fn drop(&mut self) {
        let _ = self.flush_batch();
        let _ = self.writer.flush();
        if !self.keep {
            let _ = std::fs::remove_file(&self.path);
        }
    }
}
