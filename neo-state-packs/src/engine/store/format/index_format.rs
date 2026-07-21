use super::*;

impl IndexRun {
    /// Filter gate plus fence-guided record probe. The caller has already
    /// proven the key is inside this run's key range.
    pub(super) fn probe_membership(
        &self,
        key: &[u8; PACK_KEY_BYTES],
        hash: u64,
        cursor: Option<&mut usize>,
    ) -> Result<Option<IndexEntry>> {
        let map = self.lookup_map.as_ref().unwrap_or(&self.map);
        if !self.filter.maybe_contains_hash(map, hash)? {
            return Ok(None);
        }
        self.probe(key, cursor)
    }

    /// Fence binary search (or gallop from the batch cursor) plus one
    /// in-memory search of the covering mapped record window.
    fn probe(
        &self,
        key: &[u8; PACK_KEY_BYTES],
        cursor: Option<&mut usize>,
    ) -> Result<Option<IndexEntry>> {
        let truncated = truncate_key(key);
        let partition = match cursor {
            Some(hint) => {
                let partition = gallop_partition_point(&self.fences, &truncated, *hint);
                *hint = partition;
                partition
            }
            None => self.fences.partition_point(|fence| fence < &truncated),
        };
        let fence_end =
            partition + self.fences[partition..].partition_point(|fence| fence <= &truncated);
        let record_count =
            usize::try_from(self.record_count).context("record count does not fit usize")?;
        let first = (partition.saturating_sub(1) * FENCE_INTERVAL).min(record_count);
        let last = (fence_end * FENCE_INTERVAL).min(record_count);
        if first >= last {
            return Ok(None);
        }
        let records_start =
            usize::try_from(self.records_offset).context("records offset does not fit usize")?;
        // Both point and sorted-batch probes touch sparse record windows.
        // Sorted cursors are monotone but still jump across a very large run;
        // using the ordinary map here caused multi-megabyte readahead per
        // verified hit. Sequential verification and compaction access `map`
        // directly and retain their ordinary readahead policy.
        let map = self.lookup_map.as_ref().unwrap_or(&self.map);
        let window = map
            .as_slice()
            .get(records_start + first * INDEX_RECORD_LEN..records_start + last * INDEX_RECORD_LEN)
            .context("fence probe window outside the run")?;
        let mut low = 0usize;
        let mut high = window.len() / INDEX_RECORD_LEN;
        while low < high {
            let mid = low + (high - low) / 2;
            let record_key: &[u8; PACK_KEY_BYTES] = window
                [mid * INDEX_RECORD_LEN..mid * INDEX_RECORD_LEN + PACK_KEY_BYTES]
                .try_into()
                .expect("record key");
            if record_key <= key {
                low = mid + 1;
            } else {
                high = mid;
            }
        }
        if low == 0 {
            return Ok(None);
        }
        let entry = decode_record(&window[(low - 1) * INDEX_RECORD_LEN..low * INDEX_RECORD_LEN])?;
        if &entry.key != key {
            return Ok(None);
        }
        Ok(Some(entry))
    }
}

impl RunFilter {
    pub(super) fn maybe_contains_hash(&self, map: &Mmap, hash: u64) -> Result<bool> {
        match self {
            Self::Xor16(filter) => Ok(filter.maybe_contains_hash(hash)),
            Self::BlockedBloom {
                seed,
                probes,
                offset,
                bytes,
            } => {
                let start = usize::try_from(*offset).context("Bloom offset does not fit usize")?;
                let len = usize::try_from(*bytes).context("Bloom length does not fit usize")?;
                let end = start.checked_add(len).context("Bloom range overflows")?;
                let section = map
                    .as_slice()
                    .get(start..end)
                    .context("mapped Bloom section lies outside the index run")?;
                Ok(blocked_bloom_maybe_contains_hash(
                    section, *seed, *probes, hash,
                ))
            }
        }
    }

    pub(super) fn memory_bytes(&self) -> u64 {
        match self {
            Self::Xor16(filter) => filter.memory_bytes(),
            // V4 Bloom bytes remain mmap-backed; only this fixed metadata is
            // decoded. The build-time bitset is charged as transient workspace.
            Self::BlockedBloom { .. } => 32,
        }
    }
}

/// Galloping partition point over sorted fences. `hint` must not exceed the
/// true partition point (monotone sorted batch queries guarantee this).
fn gallop_partition_point(
    fences: &[[u8; FENCE_KEY_BYTES]],
    target: &[u8; FENCE_KEY_BYTES],
    hint: usize,
) -> usize {
    debug_assert!(hint <= fences.len());
    let mut lower = hint;
    let mut step = 1usize;
    while lower + step < fences.len() && fences[lower + step] < *target {
        lower += step;
        step <<= 1;
    }
    let upper = (lower + step).min(fences.len());
    lower + fences[lower..upper].partition_point(|fence| fence < target)
}

pub(super) fn truncate_key(key: &[u8; PACK_KEY_BYTES]) -> [u8; FENCE_KEY_BYTES] {
    key[..FENCE_KEY_BYTES].try_into().expect("fence key prefix")
}

/// Big-endian leading u64 of a key, order-equivalent to byte comparison.
pub(super) fn key_prefix(key: &[u8; PACK_KEY_BYTES]) -> u64 {
    u64::from_be_bytes(key[..8].try_into().expect("key prefix"))
}

pub(super) fn distinct_keys(entries: &[IndexEntry]) -> Vec<[u8; PACK_KEY_BYTES]> {
    let mut keys = Vec::with_capacity(entries.len());
    for entry in entries {
        if keys.last() != Some(&entry.key) {
            keys.push(entry.key);
        }
    }
    keys
}

pub(super) fn build_fences(entries: &[IndexEntry]) -> Vec<[u8; FENCE_KEY_BYTES]> {
    (0..entries.len())
        .step_by(FENCE_INTERVAL)
        .map(|start| truncate_key(&entries[start].key))
        .collect()
}

pub(super) fn filter_seed(epoch: u64) -> u64 {
    epoch
        .wrapping_mul(0xA076_1D64_78BD_642F)
        .wrapping_add(0xE703_7ED1_A0B4_28DB)
}

/// Resident structured bytes for one run: fences, filter, and metadata.
/// Index records are never decoded into memory, so they are not charged.
pub(super) fn run_structured_bytes(records: usize, distinct: usize) -> Result<u64> {
    let fences = u64::try_from(records.div_ceil(FENCE_INTERVAL) * FENCE_KEY_BYTES)
        .context("fence bytes do not fit u64")?;
    let filter =
        u64::try_from(filter_capacity(distinct) * 2).context("filter bytes do not fit u64")?;
    fences
        .checked_add(filter)
        .and_then(|total| total.checked_add(RUN_METADATA_BYTES))
        .context("structured index bytes overflow")
}

/// Encodes one immutable sorted run (format v3): header, sparse fences, the
/// xor16 membership filter, then the v1 record section. Everything before
/// the records is derived data rebuilt at publish time. The v3 structure
/// digest binds every lookup-routing header field plus the exact serialized
/// fences and filter before either accelerator may be trusted.
pub(super) fn encode_index_run(
    epoch: u64,
    entries: &[IndexEntry],
    fences: &[[u8; FENCE_KEY_BYTES]],
    filter: &XorFilter,
    min_key: &[u8; PACK_KEY_BYTES],
    max_key: &[u8; PACK_KEY_BYTES],
) -> Result<(Vec<u8>, [u8; 32])> {
    let record_bytes = entries
        .len()
        .checked_mul(INDEX_RECORD_LEN)
        .context("index run size overflows usize")?;
    let mut records = Vec::with_capacity(record_bytes);
    for entry in entries {
        records.extend_from_slice(&entry.key);
        records.extend_from_slice(&entry.sequence.to_le_bytes());
        records.extend_from_slice(&entry.value_offset.to_le_bytes());
        records.extend_from_slice(&entry.value_len.to_le_bytes());
        records.push(u8::from(entry.tombstone));
    }
    let records_sha256 = digest(&records);
    let mut header = [0u8; INDEX_HEADER_LEN];
    header[0..8].copy_from_slice(INDEX_MAGIC);
    header[8..12].copy_from_slice(&PACK_INDEX_FORMAT_VERSION.to_le_bytes());
    header[12..16].copy_from_slice(&(INDEX_HEADER_LEN as u32).to_le_bytes());
    header[16..24].copy_from_slice(&epoch.to_le_bytes());
    header[24..32].copy_from_slice(
        &u64::try_from(entries.len())
            .context("index entry count does not fit u64")?
            .to_le_bytes(),
    );
    header[32..64].copy_from_slice(&records_sha256);
    header[64..68].copy_from_slice(
        &u32::try_from(fences.len())
            .context("fence count does not fit u32")?
            .to_le_bytes(),
    );
    header[68..72].copy_from_slice(&(FENCE_INTERVAL as u32).to_le_bytes());
    header[72..80].copy_from_slice(&filter.seed().to_le_bytes());
    header[80..84].copy_from_slice(
        &u32::try_from(filter.fingerprint_count())
            .context("filter size does not fit u32")?
            .to_le_bytes(),
    );
    header[84..88].copy_from_slice(&FILTER_FINGERPRINT_BITS.to_le_bytes());
    header[88..121].copy_from_slice(min_key);
    header[121..154].copy_from_slice(max_key);
    let filter_bytes = filter.encode();
    let mut output = Vec::with_capacity(
        INDEX_HEADER_LEN + fences.len() * FENCE_KEY_BYTES + filter_bytes.len() + records.len(),
    );
    output.extend_from_slice(&header);
    for fence in fences {
        output.extend_from_slice(fence);
    }
    output.extend_from_slice(&filter_bytes);
    let structure_end = output.len();
    let structure_sha256 = index_structure_digest(
        XOR_INDEX_RUN_FORMAT_VERSION,
        output[..INDEX_HEADER_LEN]
            .try_into()
            .expect("index header length"),
        &output[INDEX_HEADER_LEN..structure_end],
    )?;
    output[INDEX_STRUCTURE_SHA256_START..INDEX_STRUCTURE_SHA256_END]
        .copy_from_slice(&structure_sha256);
    let tag = digest(&output[..INDEX_HEADER_TAG_START]);
    output[INDEX_HEADER_TAG_START..INDEX_HEADER_LEN].copy_from_slice(&tag[..4]);
    output.extend_from_slice(&records);
    Ok((output, records_sha256))
}

/// Reads a v3 run header, fences, and filter into memory and performs the
/// integrity and structure checks before either accelerator can affect a
/// lookup. Records are never decoded here.
pub(super) fn map_random_if_enabled(
    file: &File,
    len: u64,
    path: &Path,
    options: PackStoreOptions,
) -> Result<Option<Mmap>> {
    options
        .random_point_mmap
        .then(|| Mmap::map_random(file, len, path))
        .transpose()
}

#[cfg(test)]
pub(super) fn read_index_run(path: &Path) -> Result<IndexRun> {
    read_index_run_with_options(path, PackStoreOptions::default())
}

pub(super) fn read_index_run_with_options(
    path: &Path,
    options: PackStoreOptions,
) -> Result<IndexRun> {
    let file = File::open(path).with_context(|| format!("open index run {}", path.display()))?;
    let file_len = file
        .metadata()
        .with_context(|| format!("stat index run {}", path.display()))?
        .len();
    ensure!(
        file_len >= (INDEX_HEADER_LEN + INDEX_RECORD_LEN) as u64,
        "short index run {}",
        path.display()
    );
    let mut header = [0u8; INDEX_HEADER_LEN];
    file.read_exact_at(&mut header, 0)
        .with_context(|| format!("read index header {}", path.display()))?;
    ensure!(
        &header[0..8] == INDEX_MAGIC,
        "invalid index magic in {}",
        path.display()
    );
    let format_version = u32_at(&header, 8)?;
    if !matches!(
        format_version,
        XOR_INDEX_RUN_FORMAT_VERSION | PACK_INDEX_RUN_FORMAT_VERSION
    ) {
        return Err(PackStoreError::unsupported_version(
            PackStoreArtifact::IndexRun,
            format_version,
            &[XOR_INDEX_RUN_FORMAT_VERSION, PACK_INDEX_RUN_FORMAT_VERSION],
        )
        .into());
    }
    ensure!(
        u32_at(&header, 12)? as usize == INDEX_HEADER_LEN,
        "invalid index header length"
    );
    let tag = digest(&header[..INDEX_HEADER_TAG_START]);
    ensure!(
        header[INDEX_HEADER_TAG_START..INDEX_HEADER_LEN] == tag[..4],
        "index header tag mismatch in {}",
        path.display()
    );
    match format_version {
        XOR_INDEX_RUN_FORMAT_VERSION => ensure!(
            header[INDEX_STRUCTURE_SHA256_END..INDEX_HEADER_TAG_START]
                .iter()
                .all(|byte| *byte == 0),
            "index header reserved bytes are non-zero in {}",
            path.display()
        ),
        PACK_INDEX_RUN_FORMAT_VERSION => ensure!(
            u16_at(&header, INDEX_STRUCTURE_SHA256_END)? == BLOOM_HASH_PROBES as u16,
            "unsupported blocked Bloom probe count"
        ),
        _ => unreachable!("validated physical run version"),
    }
    let epoch = u64_at(&header, 16)?;
    let record_count = u64_at(&header, 24)?;
    ensure!(record_count > 0, "empty index run {}", path.display());
    let records_sha256: [u8; 32] = header[32..64].try_into().expect("records checksum");
    let fence_count = usize::try_from(u32_at(&header, 64)?).context("fence count overflows")?;
    ensure!(
        u32_at(&header, 68)? as usize == FENCE_INTERVAL,
        "unsupported fence interval"
    );
    let records = usize::try_from(record_count).context("index count does not fit usize")?;
    ensure!(
        fence_count == records.div_ceil(FENCE_INTERVAL),
        "fence count mismatch in {}",
        path.display()
    );
    let seed = u64_at(&header, 72)?;
    let mut min_key = [0u8; PACK_KEY_BYTES];
    min_key.copy_from_slice(&header[88..121]);
    let mut max_key = [0u8; PACK_KEY_BYTES];
    max_key.copy_from_slice(&header[121..154]);
    ensure!(
        min_key <= max_key,
        "inverted index key range in {}",
        path.display()
    );
    let fence_bytes = fence_count
        .checked_mul(FENCE_KEY_BYTES)
        .context("index fence byte length overflows")?;
    let (filter_bytes, filter_parameter) = match format_version {
        XOR_INDEX_RUN_FORMAT_VERSION => {
            let filter_parameter = u32_at(&header, 84)?;
            ensure!(
                filter_parameter == FILTER_FINGERPRINT_BITS,
                "unsupported xor filter fingerprint width"
            );
            let bytes = usize::try_from(u32_at(&header, 80)?)
                .context("xor filter size overflows")?
                .checked_mul(2)
                .context("xor filter byte length overflows")?;
            (bytes, filter_parameter)
        }
        PACK_INDEX_RUN_FORMAT_VERSION => {
            let bytes =
                usize::try_from(u64_at(&header, 80)?).context("Bloom size does not fit usize")?;
            ensure!(
                u64::try_from(bytes).context("Bloom size does not fit u64")?
                    == blocked_bloom_bytes(record_count)?,
                "blocked Bloom geometry does not match the unique record count"
            );
            (bytes, BLOOM_HASH_PROBES)
        }
        _ => unreachable!("validated physical run version"),
    };
    let records_offset_usize = INDEX_HEADER_LEN
        .checked_add(fence_bytes)
        .and_then(|offset| offset.checked_add(filter_bytes))
        .context("index records offset overflows")?;
    let records_offset =
        u64::try_from(records_offset_usize).context("index records offset does not fit u64")?;
    let expected_len = records_offset
        .checked_add(
            record_count
                .checked_mul(INDEX_RECORD_LEN as u64)
                .context("index length overflows")?,
        )
        .context("index length overflows")?;
    ensure!(
        file_len == expected_len,
        "index run length mismatch in {}",
        path.display()
    );
    let map = Mmap::map(&file, file_len, path)?;
    let structure = map
        .as_slice()
        .get(INDEX_HEADER_LEN..records_offset_usize)
        .context("mapped index structure lies outside the run")?;
    ensure!(
        index_structure_digest(format_version, &header, structure)?.as_slice()
            == &header[INDEX_STRUCTURE_SHA256_START..INDEX_STRUCTURE_SHA256_END],
        "index structure checksum mismatch in {}",
        path.display()
    );
    let mut fences = Vec::with_capacity(fence_count);
    for chunk in structure[..fence_bytes].chunks_exact(FENCE_KEY_BYTES) {
        fences.push(chunk.try_into().expect("fence chunk"));
    }
    ensure!(
        fences.windows(2).all(|pair| pair[0] <= pair[1]),
        "index fences are not sorted"
    );
    let filter_section = &structure[fence_bytes..];
    let filter = match format_version {
        XOR_INDEX_RUN_FORMAT_VERSION => RunFilter::Xor16(
            XorFilter::decode(seed, filter_section).context("decode run membership filter")?,
        ),
        PACK_INDEX_RUN_FORMAT_VERSION => {
            validate_blocked_bloom(filter_section, filter_parameter)?;
            RunFilter::BlockedBloom {
                seed,
                probes: filter_parameter,
                offset: u64::try_from(INDEX_HEADER_LEN + fence_bytes)
                    .context("Bloom offset does not fit u64")?,
                bytes: u64::try_from(filter_bytes).context("Bloom length does not fit u64")?,
            }
        }
        _ => unreachable!("validated physical run version"),
    };
    let records = map
        .as_slice()
        .get(records_offset_usize..)
        .context("mapped index records lie outside the run")?;
    let first = decode_record(&records[..INDEX_RECORD_LEN])?;
    let last = decode_record(&records[records.len() - INDEX_RECORD_LEN..])?;
    ensure!(
        first.key == min_key && last.key == max_key,
        "index key range does not match its records in {}",
        path.display()
    );
    ensure!(
        fences.first() == Some(&truncate_key(&first.key)),
        "first fence does not match the first record in {}",
        path.display()
    );
    let lookup_map = map_random_if_enabled(&file, file_len, path, options)?;
    drop(file);
    let memory_bytes = u64::try_from(fence_bytes)
        .context("structured bytes overflow")?
        .checked_add(filter.memory_bytes())
        .and_then(|total| total.checked_add(RUN_METADATA_BYTES))
        .context("structured bytes overflow")?;
    Ok(IndexRun {
        format_version,
        epoch,
        record_count,
        map,
        lookup_map,
        records_offset,
        file_bytes: file_len,
        min_key,
        max_key,
        min_prefix: key_prefix(&min_key),
        max_prefix: key_prefix(&max_key),
        fences,
        filter,
        records_sha256,
        memory_bytes,
    })
}

/// Fully re-verifies the records checksum of one committed run. Recovery
/// applies this to every run in the selected manifest so an older corrupted
/// index cannot be exposed until an explicit rebuild succeeds.
pub(super) fn verify_run(run: &IndexRun) -> Result<()> {
    let records_len = usize::try_from(
        run.record_count
            .checked_mul(INDEX_RECORD_LEN as u64)
            .context("index run records length overflows")?,
    )
    .context("index run records length does not fit usize")?;
    let records_start =
        usize::try_from(run.records_offset).context("records offset does not fit usize")?;
    let records = run
        .map
        .as_slice()
        .get(records_start..records_start + records_len)
        .context("read committed index run records")?;
    ensure!(
        digest(records).as_slice() == run.records_sha256,
        "index records checksum mismatch in committed run"
    );
    Ok(())
}

/// Encodes and atomically publishes one fresh run file (tmp + sync + rename
/// + directory sync), then reads it back through the validating reader so
/// every published run is structurally verified before use.
pub(super) fn publish_fresh_run(
    entries: &[IndexEntry],
    epoch: u64,
    runs_dir: &Path,
    file_name: &str,
    options: PackStoreOptions,
) -> Result<IndexRun> {
    ensure!(!entries.is_empty(), "cannot publish an empty index run");
    let min_key = entries.first().expect("non-empty run").key;
    let max_key = entries.last().expect("non-empty run").key;
    let fences = build_fences(entries);
    let keys = distinct_keys(entries);
    let filter =
        XorFilter::build(&keys, filter_seed(epoch)).context("build run membership filter")?;
    let (index_bytes, _) = encode_index_run(epoch, entries, &fences, &filter, &min_key, &max_key)?;
    let final_path = runs_dir.join(file_name);
    let temp_path = runs_dir.join(format!("{file_name}.tmp"));
    let mut file = OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&temp_path)
        .with_context(|| format!("create index run {}", temp_path.display()))?;
    file.write_all(&index_bytes)
        .with_context(|| format!("write index run {}", temp_path.display()))?;
    file.sync_data()
        .with_context(|| format!("sync index run {}", temp_path.display()))?;
    drop(file);
    fs::rename(&temp_path, &final_path).with_context(|| {
        format!(
            "publish index run {} as {}",
            temp_path.display(),
            final_path.display()
        )
    })?;
    sync_directory(runs_dir)?;
    read_index_run_with_options(&final_path, options)
}
