//! Bounded k-way merge for immutable sorted index runs.
//!
//! Compaction inputs are already ordered by key and then row sequence. A
//! cursor consumes a complete same-key group before exposing it to the heap,
//! so the newest row in one epoch cannot be hidden behind an older row from
//! that same run. Only one group per input is resident.

use super::mmap::Mmap;
use super::store::PackSegmentId;
use crate::PACK_KEY_BYTES;
use anyhow::{Context, Result, ensure};
use sha2::{Digest, Sha256};
use std::cmp::Ordering;
use std::collections::BinaryHeap;

/// Fixed production v5 index-record length.
pub(super) const INDEX_RECORD_LEN: usize = 64;
const INDEX_KIND_OFFSET: usize = PACK_KEY_BYTES;
const INDEX_SEQUENCE_OFFSET: usize = 36;
const INDEX_SEGMENT_OFFSET: usize = 40;
const INDEX_VALUE_OFFSET: usize = 48;
const INDEX_VALUE_LEN_OFFSET: usize = 56;
const INDEX_TRAILING_RESERVED_OFFSET: usize = 60;
const INPUT_HASH_CHUNK_BYTES: usize = 1024 * 1024;
const OUTPUT_HASH_CHUNK_BYTES: usize = 64 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct IndexEntry {
    pub(super) key: [u8; PACK_KEY_BYTES],
    pub(super) sequence: u32,
    pub(super) segment_id: PackSegmentId,
    pub(super) value_offset: u64,
    pub(super) value_len: u32,
    pub(super) tombstone: bool,
}

pub(super) struct MergeSource<'a> {
    pub(super) max_epoch: u64,
    pub(super) record_count: u64,
    pub(super) records: &'a [u8],
    pub(super) records_sha256: [u8; 32],
    pub(super) mapping: Option<&'a Mmap>,
    pub(super) records_offset: usize,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct MergeEvidence {
    pub(super) output_records: u64,
    pub(super) min_key: [u8; PACK_KEY_BYTES],
    pub(super) max_key: [u8; PACK_KEY_BYTES],
    pub(super) records_sha256: [u8; 32],
}

/// Merge-walks sorted sources with newest-epoch/newest-sequence semantics.
/// Every raw input record is checksummed and structurally validated before the
/// pass succeeds. The visitor receives each unique-key winner and its canonical
/// fixed-size encoding in ascending key order.
pub(super) fn merge_sorted_runs(
    sources: &[MergeSource<'_>],
    mut visit: impl FnMut(u64, &IndexEntry, &[u8; INDEX_RECORD_LEN]) -> Result<()>,
) -> Result<MergeEvidence> {
    ensure!(!sources.is_empty(), "merge requires at least one input");
    let mut cursors: Vec<_> = sources
        .iter()
        .enumerate()
        .map(|(source_index, source)| RunCursor::new(source_index, source))
        .collect::<Result<_>>()?;
    let mut heap = BinaryHeap::with_capacity(cursors.len());
    for cursor in &mut cursors {
        if let Some(group) = cursor.next_group()? {
            heap.push(group);
        }
    }

    let mut output_records = 0u64;
    let mut min_key = None;
    let mut max_key = None;
    let mut output_hasher = BufferedHasher::new();
    while let Some(mut winner) = heap.pop() {
        let key = winner.entry.key;
        advance_cursor(&mut cursors, &mut heap, winner.source_index)?;
        while heap
            .peek()
            .is_some_and(|candidate| candidate.entry.key == key)
        {
            let candidate = heap.pop().expect("peeked merge candidate");
            advance_cursor(&mut cursors, &mut heap, candidate.source_index)?;
            if candidate.is_newer_than(&winner) {
                winner = candidate;
            } else if candidate.same_version(&winner) {
                ensure!(
                    candidate.entry == winner.entry,
                    "overlapping index runs disagree at the same epoch and sequence"
                );
            }
        }

        let encoded = encode_record(&winner.entry);
        visit(output_records, &winner.entry, &encoded)?;
        output_hasher.update(&encoded);
        min_key.get_or_insert(key);
        max_key = Some(key);
        output_records = output_records
            .checked_add(1)
            .context("merged output record count overflows")?;
    }
    ensure!(output_records > 0, "compaction produced an empty index run");
    for cursor in cursors {
        cursor.verify_complete()?;
    }
    Ok(MergeEvidence {
        output_records,
        min_key: min_key.expect("non-empty merge minimum"),
        max_key: max_key.expect("non-empty merge maximum"),
        records_sha256: output_hasher.finalize(),
    })
}

fn advance_cursor(
    cursors: &mut [RunCursor<'_>],
    heap: &mut BinaryHeap<HeapGroup>,
    source_index: usize,
) -> Result<()> {
    if let Some(group) = cursors[source_index].next_group()? {
        heap.push(group);
    }
    Ok(())
}

struct RunCursor<'a> {
    source_index: usize,
    max_epoch: u64,
    expected_records: u64,
    expected_sha256: [u8; 32],
    records: &'a [u8],
    position: usize,
    consumed: u64,
    hasher: Sha256,
    hashed_until: usize,
    previous: Option<IndexEntry>,
    mapping: Option<&'a Mmap>,
    records_base: usize,
    release_start: usize,
}

impl<'a> RunCursor<'a> {
    fn new(source_index: usize, source: &MergeSource<'a>) -> Result<Self> {
        ensure!(source.record_count > 0, "compaction input run is empty");
        let expected_bytes = usize::try_from(source.record_count)
            .context("compaction input record count does not fit usize")?
            .checked_mul(INDEX_RECORD_LEN)
            .context("compaction input byte length overflows")?;
        ensure!(
            source.records.len() == expected_bytes,
            "compaction input record section length mismatch"
        );
        Ok(Self {
            source_index,
            max_epoch: source.max_epoch,
            expected_records: source.record_count,
            expected_sha256: source.records_sha256,
            records: source.records,
            position: 0,
            consumed: 0,
            hasher: Sha256::new(),
            hashed_until: 0,
            previous: None,
            mapping: source.mapping,
            records_base: source.records_offset,
            release_start: source.records_offset,
        })
    }

    fn next_group(&mut self) -> Result<Option<HeapGroup>> {
        if self.position == self.records.len() {
            return Ok(None);
        }
        let first = self.consume_record()?;
        let key = first.key;
        let mut winner = first;
        while self.position < self.records.len() {
            let next_key: &[u8; PACK_KEY_BYTES] = self.records
                [self.position..self.position + PACK_KEY_BYTES]
                .try_into()
                .expect("fixed record key");
            if next_key != &key {
                break;
            }
            winner = self.consume_record()?;
        }
        Ok(Some(HeapGroup {
            source_index: self.source_index,
            max_epoch: self.max_epoch,
            entry: winner,
        }))
    }

    fn consume_record(&mut self) -> Result<IndexEntry> {
        let end = self
            .position
            .checked_add(INDEX_RECORD_LEN)
            .context("compaction input cursor overflows")?;
        let raw = self
            .records
            .get(self.position..end)
            .context("truncated compaction input record")?;
        let entry = decode_record(raw)?;
        if let Some(previous) = self.previous {
            ensure!(
                previous.key < entry.key
                    || (previous.key == entry.key && previous.sequence < entry.sequence),
                "compaction input records are not ordered by key and sequence"
            );
        }
        self.position = end;
        if self.position - self.hashed_until >= INPUT_HASH_CHUNK_BYTES {
            self.hasher
                .update(&self.records[self.hashed_until..self.position]);
            self.hashed_until = self.position;
            self.release_consumed_pages()?;
        }
        self.consumed = self
            .consumed
            .checked_add(1)
            .context("compaction input record count overflows")?;
        self.previous = Some(entry);
        Ok(entry)
    }

    fn verify_complete(mut self) -> Result<()> {
        ensure!(
            self.position == self.records.len() && self.consumed == self.expected_records,
            "compaction input cursor did not consume the complete run"
        );
        if self.hashed_until < self.position {
            self.hasher
                .update(&self.records[self.hashed_until..self.position]);
        }
        self.release_consumed_pages()?;
        ensure!(
            <[u8; 32]>::from(self.hasher.finalize()) == self.expected_sha256,
            "compaction input records checksum mismatch"
        );
        Ok(())
    }

    fn release_consumed_pages(&mut self) -> Result<()> {
        let Some(mapping) = self.mapping else {
            return Ok(());
        };
        let absolute_end = self
            .records_base
            .checked_add(self.position)
            .context("compaction reclaim range overflows")?;
        self.release_start = mapping.advise_dontneed(self.release_start, absolute_end)?;
        Ok(())
    }
}

struct BufferedHasher {
    hasher: Sha256,
    pending: Vec<u8>,
}

impl BufferedHasher {
    fn new() -> Self {
        Self {
            hasher: Sha256::new(),
            pending: Vec::with_capacity(OUTPUT_HASH_CHUNK_BYTES),
        }
    }

    fn update(&mut self, bytes: &[u8]) {
        if self.pending.len() + bytes.len() > OUTPUT_HASH_CHUNK_BYTES {
            self.hasher.update(&self.pending);
            self.pending.clear();
        }
        self.pending.extend_from_slice(bytes);
    }

    fn finalize(mut self) -> [u8; 32] {
        self.hasher.update(&self.pending);
        self.hasher.finalize().into()
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct HeapGroup {
    source_index: usize,
    max_epoch: u64,
    entry: IndexEntry,
}

impl HeapGroup {
    fn is_newer_than(&self, other: &Self) -> bool {
        (self.max_epoch, self.entry.sequence) > (other.max_epoch, other.entry.sequence)
    }

    fn same_version(&self, other: &Self) -> bool {
        (self.max_epoch, self.entry.sequence) == (other.max_epoch, other.entry.sequence)
    }
}

impl Ord for HeapGroup {
    fn cmp(&self, other: &Self) -> Ordering {
        // BinaryHeap is a max-heap, so reverse the key ordering. Source index
        // makes otherwise-equal groups deterministic without affecting winner
        // selection, which is performed explicitly after all equal keys pop.
        other
            .entry
            .key
            .cmp(&self.entry.key)
            .then_with(|| other.source_index.cmp(&self.source_index))
    }
}

impl PartialOrd for HeapGroup {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

pub(super) fn encode_record(entry: &IndexEntry) -> [u8; INDEX_RECORD_LEN] {
    debug_assert!(
        !entry.tombstone
            || (entry.segment_id == PackSegmentId::INITIAL
                && entry.value_offset == 0
                && entry.value_len == 0)
    );
    let mut record = [0u8; INDEX_RECORD_LEN];
    record[..PACK_KEY_BYTES].copy_from_slice(&entry.key);
    record[INDEX_KIND_OFFSET] = u8::from(!entry.tombstone);
    record[INDEX_SEQUENCE_OFFSET..INDEX_SEGMENT_OFFSET]
        .copy_from_slice(&entry.sequence.to_le_bytes());
    if !entry.tombstone {
        record[INDEX_SEGMENT_OFFSET..INDEX_VALUE_OFFSET]
            .copy_from_slice(&entry.segment_id.get().to_le_bytes());
        record[INDEX_VALUE_OFFSET..INDEX_VALUE_LEN_OFFSET]
            .copy_from_slice(&entry.value_offset.to_le_bytes());
        record[INDEX_VALUE_LEN_OFFSET..INDEX_TRAILING_RESERVED_OFFSET]
            .copy_from_slice(&entry.value_len.to_le_bytes());
    }
    record
}

pub(super) fn decode_record(record: &[u8]) -> Result<IndexEntry> {
    ensure!(record.len() == INDEX_RECORD_LEN, "short index record");
    let mut key = [0u8; PACK_KEY_BYTES];
    key.copy_from_slice(&record[..PACK_KEY_BYTES]);
    ensure!(
        key[0] == 0xf0,
        "index record key is outside the MPT node namespace"
    );
    let kind = record[INDEX_KIND_OFFSET];
    ensure!(kind <= 1, "invalid index operation kind {kind}");
    ensure!(
        record[INDEX_KIND_OFFSET + 1..INDEX_SEQUENCE_OFFSET]
            .iter()
            .all(|byte| *byte == 0),
        "index record reserved bytes are non-zero"
    );
    ensure!(
        record[INDEX_TRAILING_RESERVED_OFFSET..]
            .iter()
            .all(|byte| *byte == 0),
        "index record trailing reserved bytes are non-zero"
    );
    let segment_id = PackSegmentId::new(u64::from_le_bytes(
        record[INDEX_SEGMENT_OFFSET..INDEX_VALUE_OFFSET]
            .try_into()
            .expect("fixed segment identity"),
    ));
    let value_offset = u64::from_le_bytes(
        record[INDEX_VALUE_OFFSET..INDEX_VALUE_LEN_OFFSET]
            .try_into()
            .expect("fixed value offset"),
    );
    let value_len = u32::from_le_bytes(
        record[INDEX_VALUE_LEN_OFFSET..INDEX_TRAILING_RESERVED_OFFSET]
            .try_into()
            .expect("fixed value length"),
    );
    ensure!(
        kind == 1 || (segment_id == PackSegmentId::INITIAL && value_offset == 0 && value_len == 0),
        "tombstone index record carries a non-zero value location"
    );
    Ok(IndexEntry {
        key,
        sequence: u32::from_le_bytes(
            record[INDEX_SEQUENCE_OFFSET..INDEX_SEGMENT_OFFSET]
                .try_into()
                .expect("fixed sequence"),
        ),
        segment_id,
        value_offset,
        value_len,
        tombstone: kind == 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(key: u8, sequence: u32, value: u8) -> IndexEntry {
        let mut bytes = [key; PACK_KEY_BYTES];
        bytes[0] = 0xf0;
        IndexEntry {
            key: bytes,
            sequence,
            segment_id: PackSegmentId::INITIAL,
            value_offset: u64::from(value),
            value_len: 1,
            tombstone: false,
        }
    }

    fn source(epoch: u64, entries: &[IndexEntry]) -> (Vec<u8>, [u8; 32]) {
        let bytes: Vec<u8> = entries.iter().flat_map(encode_record).collect();
        let checksum = Sha256::digest(&bytes).into();
        let _ = epoch;
        (bytes, checksum)
    }

    #[test]
    fn grouped_merge_uses_newest_sequence_then_newest_epoch() {
        let old = [entry(1, 0, 10), entry(2, 1, 20), entry(2, 4, 21)];
        let new = [entry(2, 0, 30), entry(3, 1, 40)];
        let (old_bytes, old_sha) = source(4, &old);
        let (new_bytes, new_sha) = source(9, &new);
        let sources = [
            MergeSource {
                max_epoch: 4,
                record_count: old.len() as u64,
                records: &old_bytes,
                records_sha256: old_sha,
                mapping: None,
                records_offset: 0,
            },
            MergeSource {
                max_epoch: 9,
                record_count: new.len() as u64,
                records: &new_bytes,
                records_sha256: new_sha,
                mapping: None,
                records_offset: 0,
            },
        ];
        let mut winners = Vec::new();
        let evidence = merge_sorted_runs(&sources, |_, entry, _| {
            winners.push(*entry);
            Ok(())
        })
        .expect("merge sorted runs");
        assert_eq!(evidence.output_records, 3);
        assert_eq!(
            winners
                .iter()
                .map(|winner| winner.value_offset)
                .collect::<Vec<_>>(),
            vec![10, 30, 40]
        );
    }

    #[test]
    fn merge_rejects_bad_order_checksum_and_tombstone_encoding() {
        let unsorted = [entry(2, 0, 1), entry(1, 1, 2)];
        let peer = [entry(3, 0, 3)];
        let (unsorted_bytes, unsorted_sha) = source(0, &unsorted);
        let (peer_bytes, peer_sha) = source(1, &peer);
        let sources = [
            MergeSource {
                max_epoch: 0,
                record_count: 2,
                records: &unsorted_bytes,
                records_sha256: unsorted_sha,
                mapping: None,
                records_offset: 0,
            },
            MergeSource {
                max_epoch: 1,
                record_count: 1,
                records: &peer_bytes,
                records_sha256: peer_sha,
                mapping: None,
                records_offset: 0,
            },
        ];
        assert!(merge_sorted_runs(&sources, |_, _, _| Ok(())).is_err());

        let mut corrupt_sources = sources;
        corrupt_sources[0].records_sha256[0] ^= 1;
        assert!(merge_sorted_runs(&corrupt_sources, |_, _, _| Ok(())).is_err());

        let mut invalid_tombstone = encode_record(&entry(4, 0, 4));
        invalid_tombstone[INDEX_KIND_OFFSET] = 0;
        assert!(decode_record(&invalid_tombstone).is_err());
        invalid_tombstone[INDEX_VALUE_OFFSET..INDEX_VALUE_LEN_OFFSET]
            .copy_from_slice(&0u64.to_le_bytes());
        invalid_tombstone[INDEX_VALUE_LEN_OFFSET..INDEX_TRAILING_RESERVED_OFFSET]
            .copy_from_slice(&0u32.to_le_bytes());
        assert!(decode_record(&invalid_tombstone).is_ok());
        invalid_tombstone[INDEX_KIND_OFFSET] = 2;
        assert!(decode_record(&invalid_tombstone).is_err());

        let mut invalid_reserved = encode_record(&entry(4, 0, 4));
        invalid_reserved[INDEX_KIND_OFFSET + 1] = 1;
        assert!(decode_record(&invalid_reserved).is_err());

        let mut invalid_namespace = encode_record(&entry(5, 0, 5));
        invalid_namespace[0] = 0xef;
        assert!(decode_record(&invalid_namespace).is_err());
    }

    #[test]
    fn index_v5_records_bind_positions_and_canonical_kinds() {
        let mut put = entry(7, 0x4433_2211, 9);
        put.segment_id = PackSegmentId::new(0x0102_0304_0506_0708);
        put.value_offset = 0x1112_1314_1516_1718;
        put.value_len = 0x2122_2324;
        let encoded = encode_record(&put);
        let mut expected = [0u8; INDEX_RECORD_LEN];
        expected[..PACK_KEY_BYTES].copy_from_slice(&put.key);
        expected[INDEX_KIND_OFFSET] = 1;
        expected[INDEX_SEQUENCE_OFFSET..INDEX_SEGMENT_OFFSET]
            .copy_from_slice(&put.sequence.to_le_bytes());
        expected[INDEX_SEGMENT_OFFSET..INDEX_VALUE_OFFSET]
            .copy_from_slice(&put.segment_id.get().to_le_bytes());
        expected[INDEX_VALUE_OFFSET..INDEX_VALUE_LEN_OFFSET]
            .copy_from_slice(&put.value_offset.to_le_bytes());
        expected[INDEX_VALUE_LEN_OFFSET..INDEX_TRAILING_RESERVED_OFFSET]
            .copy_from_slice(&put.value_len.to_le_bytes());
        assert_eq!(encoded, expected);
        assert_eq!(decode_record(&encoded).expect("decode positioned put"), put);

        let empty_put = IndexEntry {
            value_len: 0,
            ..put
        };
        assert_eq!(
            decode_record(&encode_record(&empty_put)).expect("decode empty put"),
            empty_put
        );

        let tombstone = IndexEntry {
            segment_id: PackSegmentId::INITIAL,
            value_offset: 0,
            value_len: 0,
            tombstone: true,
            ..put
        };
        let encoded = encode_record(&tombstone);
        assert_eq!(encoded[INDEX_KIND_OFFSET], 0);
        assert!(
            encoded[INDEX_SEGMENT_OFFSET..INDEX_TRAILING_RESERVED_OFFSET]
                .iter()
                .all(|byte| *byte == 0)
        );
        assert_eq!(
            decode_record(&encoded).expect("decode tombstone"),
            tombstone
        );
    }
}
