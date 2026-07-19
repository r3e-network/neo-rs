use super::*;
use rayon::prelude::*;
use rayon::{ThreadPool, ThreadPoolBuilder};
use std::ops::Range;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::OnceLock;

pub(super) const PACK_BATCH_VALUES_PER_WORKER: usize = 256;
const MAX_PACK_VALUE_POOL_WORKERS: usize = 8;

static PACK_VALUE_POOL: OnceLock<std::result::Result<ThreadPool, String>> = OnceLock::new();

fn pack_value_pool() -> Result<&'static ThreadPool> {
    match PACK_VALUE_POOL.get_or_init(|| {
        let workers = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(MAX_PACK_VALUE_POOL_WORKERS);
        ThreadPoolBuilder::new()
            .num_threads(workers)
            .thread_name(|index| format!("neo-pack-read-{index}"))
            .build()
            .map_err(|error| error.to_string())
    }) {
        Ok(pool) => Ok(pool),
        Err(error) => Err(anyhow::anyhow!("create shared pack value pool: {error}")),
    }
}

pub(super) fn preflight_pack_value_pool(workers: usize) -> Result<()> {
    if workers > 1 {
        pack_value_pool()?;
    }
    Ok(())
}

/// Shared newest-first read path over one pinned run set; used by the live
/// store and by snapshot generations alike.
pub(super) struct ReadView<'a> {
    pub(super) runs: &'a [LiveRun],
    pub(super) ranges: &'a [RunRange],
    pub(super) pack_map: &'a Mmap,
    pub(super) lookup_pack_map: Option<&'a Mmap>,
    pub(super) batch_value_workers: usize,
}

impl ReadView<'_> {
    pub(super) fn get(&self, key: &[u8; PACK_KEY_BYTES]) -> Result<Option<Vec<u8>>> {
        self.lookup(key, None)
    }

    pub(super) fn get_bounded(
        &self,
        key: &[u8; PACK_KEY_BYTES],
        max_value_bytes: u64,
    ) -> Result<Option<Vec<u8>>> {
        let Some(entry) = self.lookup_entry(key, None)? else {
            return Ok(None);
        };
        Self::ensure_value_bound(&entry, max_value_bytes)?;
        self.entry_value(entry)
    }

    pub(super) fn get_many_sorted(
        &self,
        keys: &[[u8; PACK_KEY_BYTES]],
    ) -> Result<Vec<Option<Vec<u8>>>> {
        self.get_many_sorted_with_limits(keys, None)
    }

    pub(super) fn get_many_sorted_bounded(
        &self,
        keys: &[[u8; PACK_KEY_BYTES]],
        max_value_bytes: u64,
        max_total_value_bytes: u64,
    ) -> Result<Vec<Option<Vec<u8>>>> {
        self.get_many_sorted_with_limits(keys, Some((max_value_bytes, max_total_value_bytes)))
    }

    fn get_many_sorted_with_limits(
        &self,
        keys: &[[u8; PACK_KEY_BYTES]],
        limits: Option<(u64, u64)>,
    ) -> Result<Vec<Option<Vec<u8>>>> {
        ensure!(
            keys.windows(2).all(|pair| pair[0] <= pair[1]),
            "batch keys must be sorted"
        );
        let mut cursors = vec![0usize; self.runs.len()];
        let mut results = vec![None; keys.len()];
        let mut values = Vec::new();
        let mut total_value_bytes = 0u64;
        for (output_index, key) in keys.iter().enumerate() {
            let Some(entry) = self.lookup_entry(key, Some(&mut cursors))? else {
                continue;
            };
            if !entry.tombstone {
                if let Some((max_value_bytes, max_total_value_bytes)) = limits {
                    Self::ensure_value_bound(&entry, max_value_bytes)?;
                    total_value_bytes = total_value_bytes
                        .checked_add(u64::from(entry.value_len))
                        .context("batch value byte count overflows")?;
                    ensure!(
                        total_value_bytes <= max_total_value_bytes,
                        "batch values require {total_value_bytes} bytes, exceeding the configured limit of {max_total_value_bytes} bytes"
                    );
                }
                values.push((output_index, entry));
            }
        }

        // Hash-sorted node keys have no useful relationship with append
        // offsets. Reordering derived locations still reduces seeks and makes
        // duplicate locations adjacent, but hits remain sparse across the
        // complete pack and use the random-advised payload mapping.
        values.sort_unstable_by_key(|(_, entry)| (entry.value_offset, entry.value_len));
        let workers = self.batch_value_workers;
        if workers > 1 && values.len() >= parallel_threshold(workers) {
            // The index lookup above is deliberately single-threaded so each
            // run cursor advances in key order. Value reads are independent
            // after the global offset sort, however, and can be split across
            // a bounded number of workers without changing result order.
            // This path is opt-in because concurrent page faults can regress
            // on slower disks; the default remains sequential.
            let ranges = duplicate_safe_ranges(&values, workers);
            let pool = pack_value_pool()?;
            let chunks = catch_unwind(AssertUnwindSafe(|| {
                pool.install(|| {
                    ranges
                        .into_par_iter()
                        .map(|range| self.read_value_chunk(&values[range]))
                        .collect::<Result<Vec<_>>>()
                })
            }))
            .map_err(|_| anyhow::anyhow!("parallel pack value worker panicked"))??;
            for chunk in chunks {
                for (output_index, value) in chunk {
                    results[output_index] = Some(value);
                }
            }
        } else {
            let mut previous: Option<(u64, u32, Vec<u8>)> = None;
            for (output_index, entry) in values {
                let value = match previous.as_ref() {
                    Some((offset, length, value))
                        if *offset == entry.value_offset && *length == entry.value_len =>
                    {
                        value.clone()
                    }
                    _ => {
                        let value = self
                            .entry_value(entry)?
                            .expect("non-tombstone index entry has a value");
                        previous = Some((entry.value_offset, entry.value_len, value.clone()));
                        value
                    }
                };
                results[output_index] = Some(value);
            }
        }
        Ok(results)
    }

    fn read_value_chunk(&self, values: &[(usize, IndexEntry)]) -> Result<Vec<(usize, Vec<u8>)>> {
        let mut previous: Option<(u64, u32, Vec<u8>)> = None;
        let mut output = Vec::with_capacity(values.len());
        for (output_index, entry) in values {
            let value = match previous.as_ref() {
                Some((offset, length, value))
                    if *offset == entry.value_offset && *length == entry.value_len =>
                {
                    value.clone()
                }
                _ => {
                    let value = self
                        .entry_value(*entry)?
                        .context("non-tombstone index entry has no value")?;
                    previous = Some((entry.value_offset, entry.value_len, value.clone()));
                    value
                }
            };
            output.push((*output_index, value));
        }
        Ok(output)
    }

    fn ensure_value_bound(entry: &IndexEntry, max_value_bytes: u64) -> Result<()> {
        let value_bytes = u64::from(entry.value_len);
        ensure!(
            value_bytes <= max_value_bytes,
            "indexed value length {value_bytes} exceeds the configured limit of {max_value_bytes} bytes"
        );
        Ok(())
    }

    /// Newest-first verified lookup: the compact range directory rejects
    /// out-of-range runs with two integer compares, then the per-run xor
    /// filter proves absence without any positioned read, then the sparse
    /// fences locate the single record window that a positioned read probes.
    fn lookup(
        &self,
        key: &[u8; PACK_KEY_BYTES],
        cursors: Option<&mut [usize]>,
    ) -> Result<Option<Vec<u8>>> {
        let Some(entry) = self.lookup_entry(key, cursors)? else {
            return Ok(None);
        };
        self.entry_value(entry)
    }

    fn lookup_entry(
        &self,
        key: &[u8; PACK_KEY_BYTES],
        mut cursors: Option<&mut [usize]>,
    ) -> Result<Option<IndexEntry>> {
        let prefix = key_prefix(key);
        let mut cached_hash = None;
        for index in (0..self.runs.len()).rev() {
            let range = &self.ranges[index];
            if prefix < range.min_prefix || prefix > range.max_prefix {
                continue;
            }
            let run = &self.runs[index].run;
            // The full 33-byte boundary check runs only on a leading-u64 tie.
            if (prefix == range.min_prefix && *key < run.min_key)
                || (prefix == range.max_prefix && *key > run.max_key)
            {
                continue;
            }
            let hash = *cached_hash.get_or_insert_with(|| key_hash(key));
            let cursor = cursors.as_deref_mut().map(|cursors| &mut cursors[index]);
            let Some(entry) = run.probe_membership(key, hash, cursor)? else {
                continue;
            };
            return Ok(Some(entry));
        }
        Ok(None)
    }

    fn entry_value(&self, entry: IndexEntry) -> Result<Option<Vec<u8>>> {
        if entry.tombstone {
            return Ok(None);
        }
        let offset =
            usize::try_from(entry.value_offset).context("value offset does not fit usize")?;
        let length = usize::try_from(entry.value_len).context("value length does not fit usize")?;
        let end = offset
            .checked_add(length)
            .context("value end offset overflows")?;
        let pack_map = self.lookup_pack_map.unwrap_or(self.pack_map);
        let value = pack_map
            .as_slice()
            .get(offset..end)
            .context("indexed value outside the append pack")?;
        Ok(Some(value.to_vec()))
    }
}

const fn parallel_threshold(workers: usize) -> usize {
    workers.saturating_mul(PACK_BATCH_VALUES_PER_WORKER)
}

fn duplicate_safe_ranges(values: &[(usize, IndexEntry)], workers: usize) -> Vec<Range<usize>> {
    let chunk_size = values.len().div_ceil(workers);
    let mut ranges = Vec::with_capacity(workers);
    let mut start = 0;
    while start < values.len() {
        let mut end = start.saturating_add(chunk_size).min(values.len());
        while end < values.len()
            && values[end - 1].1.value_offset == values[end].1.value_offset
            && values[end - 1].1.value_len == values[end].1.value_len
        {
            end += 1;
        }
        ranges.push(start..end);
        start = end;
    }
    ranges
}

/// A read snapshot pinning one manifest generation. Run references and the
/// pack mapping are held directly, so reads stay valid even if compaction
/// replaces the live set; the lease additionally keeps the generation's run
/// files on disk until the snapshot is dropped and `gc` runs.
pub struct Snapshot {
    pub(super) generation: u64,
    pub(super) runs: Vec<LiveRun>,
    pub(super) ranges: Vec<RunRange>,
    pub(super) pack_map: Arc<Mmap>,
    pub(super) lookup_pack_map: Option<Arc<Mmap>>,
    pub(super) batch_value_workers: usize,
    pub(super) leases: Arc<Mutex<BTreeMap<u64, usize>>>,
}

impl Snapshot {
    /// The pinned manifest generation.
    pub const fn generation(&self) -> u64 {
        self.generation
    }

    /// Newest-committed-version point read.
    pub fn get(&self, key: &[u8; PACK_KEY_BYTES]) -> Result<Option<Vec<u8>>> {
        self.view().get(key)
    }

    /// Newest-committed-version point read that rejects an oversized indexed
    /// value before allocating its result buffer.
    pub fn get_bounded(
        &self,
        key: &[u8; PACK_KEY_BYTES],
        max_value_bytes: u64,
    ) -> Result<Option<Vec<u8>>> {
        self.view().get_bounded(key, max_value_bytes)
    }

    /// Filter-assisted k-way batch read. Keys must be sorted ascending;
    /// results align one-to-one with the input order.
    pub fn get_many_sorted(&self, keys: &[[u8; PACK_KEY_BYTES]]) -> Result<Vec<Option<Vec<u8>>>> {
        self.view().get_many_sorted(keys)
    }

    /// Sorted batch read that validates every indexed value and the complete
    /// returned-value budget before allocating any value buffer.
    pub fn get_many_sorted_bounded(
        &self,
        keys: &[[u8; PACK_KEY_BYTES]],
        max_value_bytes: u64,
        max_total_value_bytes: u64,
    ) -> Result<Vec<Option<Vec<u8>>>> {
        self.view()
            .get_many_sorted_bounded(keys, max_value_bytes, max_total_value_bytes)
    }

    fn view(&self) -> ReadView<'_> {
        ReadView {
            runs: &self.runs,
            ranges: &self.ranges,
            pack_map: &self.pack_map,
            lookup_pack_map: self.lookup_pack_map.as_deref(),
            batch_value_workers: self.batch_value_workers,
        }
    }

    pub(super) fn reclaim_random_lookup_pages(&self) -> Result<()> {
        reclaim_random_lookup_pages(&self.runs, self.lookup_pack_map.as_deref())
    }
}

pub(super) fn reclaim_random_lookup_pages(
    runs: &[LiveRun],
    lookup_pack_map: Option<&Mmap>,
) -> Result<()> {
    for live in runs {
        if let Some(map) = live.run.lookup_map.as_ref() {
            let _ = map.advise_dontneed(0, map.as_slice().len())?;
        }
    }
    if let Some(map) = lookup_pack_map {
        let _ = map.advise_dontneed(0, map.as_slice().len())?;
    }
    Ok(())
}

impl Drop for Snapshot {
    fn drop(&mut self) {
        if let Ok(mut leases) = self.leases.lock() {
            if let Some(count) = leases.get_mut(&self.generation) {
                *count -= 1;
                if *count == 0 {
                    leases.remove(&self.generation);
                }
            }
        }
    }
}
