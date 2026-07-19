use super::*;

/// Shared newest-first read path over one pinned run set; used by the live
/// store and by snapshot generations alike.
pub(super) struct ReadView<'a> {
    pub(super) runs: &'a [LiveRun],
    pub(super) ranges: &'a [RunRange],
    pub(super) pack_map: &'a Mmap,
    pub(super) lookup_pack_map: Option<&'a Mmap>,
}

impl ReadView<'_> {
    pub(super) fn get(&self, key: &[u8; PACK_KEY_BYTES]) -> Result<Option<Vec<u8>>> {
        self.lookup(key, None)
    }

    pub(super) fn get_many_sorted(
        &self,
        keys: &[[u8; PACK_KEY_BYTES]],
    ) -> Result<Vec<Option<Vec<u8>>>> {
        ensure!(
            keys.windows(2).all(|pair| pair[0] <= pair[1]),
            "batch keys must be sorted"
        );
        let mut cursors = vec![0usize; self.runs.len()];
        let mut results = vec![None; keys.len()];
        let mut values = Vec::new();
        for (output_index, key) in keys.iter().enumerate() {
            let Some(entry) = self.lookup_entry(key, Some(&mut cursors))? else {
                continue;
            };
            if !entry.tombstone {
                values.push((output_index, entry));
            }
        }

        // Hash-sorted node keys have no useful relationship with append
        // offsets. Reordering derived locations still reduces seeks and makes
        // duplicate locations adjacent, but hits remain sparse across the
        // complete pack and use the random-advised payload mapping.
        values.sort_unstable_by_key(|(_, entry)| entry.value_offset);
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
        Ok(results)
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

    /// Filter-assisted k-way batch read. Keys must be sorted ascending;
    /// results align one-to-one with the input order.
    pub fn get_many_sorted(&self, keys: &[[u8; PACK_KEY_BYTES]]) -> Result<Vec<Option<Vec<u8>>>> {
        self.view().get_many_sorted(keys)
    }

    fn view(&self) -> ReadView<'_> {
        ReadView {
            runs: &self.runs,
            ranges: &self.ranges,
            pack_map: &self.pack_map,
            lookup_pack_map: self.lookup_pack_map.as_deref(),
        }
    }
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
