use super::*;

/// Conservative peak decoded/transient index memory for the streaming build.
/// The source generation remains pinned until adoption, so its resident
/// filters and fences are included alongside the maximum-size output Bloom,
/// output fences, cursors, and two I/O buffers. Saturation defers the build.
pub(super) fn estimate_compaction_workspace_for_inputs(
    inputs: &[LiveRun],
    resident_bytes: u64,
) -> u64 {
    let input_records = inputs.iter().fold(0u64, |total, live| {
        total.saturating_add(live.run.record_count)
    });
    estimate_compaction_workspace(resident_bytes, input_records)
}

pub(super) fn estimate_compaction_workspace(resident_bytes: u64, output_records: u64) -> u64 {
    let bloom_bytes = blocked_bloom_bytes(output_records).unwrap_or(u64::MAX);
    let fence_bytes = output_records
        .div_ceil(FENCE_INTERVAL as u64)
        .saturating_mul(FENCE_KEY_BYTES as u64);
    resident_bytes
        .saturating_add(bloom_bytes)
        .saturating_add(fence_bytes)
        .saturating_add(COMPACTION_IO_BUFFER_BYTES as u64)
        .saturating_add(128 * 1024)
}

pub(super) fn ensure_compaction_workspace(estimated_bytes: u64, max_bytes: u64) -> Result<()> {
    if estimated_bytes > max_bytes {
        return Err(PackStoreError::CompactionWorkspaceExceeded {
            estimated_bytes,
            max_bytes,
        }
        .into());
    }
    Ok(())
}

pub(super) fn build_compacted_run_from_inputs(
    level: u32,
    inputs: &[LiveRun],
    runs_dir: &Path,
    random_point_mmap: bool,
    resident_index_bytes: u64,
    max_index_memory_bytes: u64,
) -> Result<PendingMerge> {
    validate_compaction_inputs(level, inputs)?;
    let estimated_workspace_bytes =
        estimate_compaction_workspace_for_inputs(inputs, resident_index_bytes);
    ensure_compaction_workspace(estimated_workspace_bytes, max_index_memory_bytes)?;
    let min_epoch = inputs.first().expect("merge inputs").min_epoch;
    let max_epoch = inputs.last().expect("merge inputs").max_epoch;
    let input_records = inputs.iter().try_fold(0u64, |total, live| {
        total
            .checked_add(live.run.record_count)
            .context("compaction input record count overflows")
    })?;
    let input_memory_bytes = inputs.iter().try_fold(0u64, |total, live| {
        total
            .checked_add(live.run.memory_bytes)
            .context("compaction input memory bytes overflow")
    })?;

    // Dedicated mappings keep sequential advice and page reclamation isolated
    // from the long-lived point-lookup mappings held by snapshots.
    let input_maps = open_compaction_input_maps(inputs, runs_dir)?;
    let sources = compaction_sources(&input_maps)?;
    let first_pass = merge_sorted_runs(&sources, |_, _, _| Ok(()))?;
    let exact_workspace =
        estimate_compaction_workspace(resident_index_bytes, first_pass.output_records);
    ensure_compaction_workspace(exact_workspace, max_index_memory_bytes)?;

    let file_name = run_file_name(level + 1, min_epoch, max_epoch);
    let run = publish_streaming_compacted_run(
        &sources,
        first_pass,
        max_epoch,
        runs_dir,
        &file_name,
        PackStoreOptions { random_point_mmap },
    )?;
    Ok(PendingMerge {
        level: level + 1,
        min_epoch,
        max_epoch,
        run,
        input_runs: u64::try_from(inputs.len()).context("merge input count overflows")?,
        input_records,
        output_records: first_pass.output_records,
        input_memory_bytes,
        inputs: Vec::new(),
        wall_ns: 0,
    })
}

fn validate_compaction_inputs(level: u32, inputs: &[LiveRun]) -> Result<()> {
    ensure!(inputs.len() >= 2, "compaction requires at least two inputs");
    for input in inputs {
        ensure!(input.level == level, "compaction input level changed");
        ensure!(
            input.min_epoch <= input.max_epoch && input.run.epoch == input.max_epoch,
            "compaction input epoch metadata is inconsistent"
        );
    }
    for pair in inputs.windows(2) {
        ensure!(
            pair[0].max_epoch.checked_add(1) == Some(pair[1].min_epoch),
            "compaction inputs do not form one contiguous manifest range"
        );
    }
    Ok(())
}

struct CompactionInputMap {
    map: Mmap,
    max_epoch: u64,
    record_count: u64,
    records_offset: usize,
    records_sha256: [u8; 32],
}

fn open_compaction_input_maps(
    inputs: &[LiveRun],
    runs_dir: &Path,
) -> Result<Vec<CompactionInputMap>> {
    let mut mappings = Vec::with_capacity(inputs.len());
    for input in inputs {
        let path = runs_dir.join(run_file_name(input.level, input.min_epoch, input.max_epoch));
        let file = File::open(&path)
            .with_context(|| format!("open compaction input {}", path.display()))?;
        let file_bytes = file
            .metadata()
            .with_context(|| format!("stat compaction input {}", path.display()))?
            .len();
        ensure!(
            file_bytes == input.run.file_bytes,
            "compaction input file length changed"
        );
        mappings.push(CompactionInputMap {
            map: Mmap::map_sequential(&file, file_bytes, &path)?,
            max_epoch: input.max_epoch,
            record_count: input.run.record_count,
            records_offset: usize::try_from(input.run.records_offset)
                .context("compaction records offset does not fit usize")?,
            records_sha256: input.run.records_sha256,
        });
    }
    Ok(mappings)
}

fn compaction_sources(mappings: &[CompactionInputMap]) -> Result<Vec<MergeSource<'_>>> {
    mappings
        .iter()
        .map(|input| {
            let record_bytes = usize::try_from(input.record_count)
                .context("compaction input record count does not fit usize")?
                .checked_mul(INDEX_RECORD_LEN)
                .context("compaction input record byte length overflows")?;
            let end = input
                .records_offset
                .checked_add(record_bytes)
                .context("compaction input record range overflows")?;
            let records = input
                .map
                .as_slice()
                .get(input.records_offset..end)
                .context("compaction input records lie outside the run")?;
            Ok(MergeSource {
                max_epoch: input.max_epoch,
                record_count: input.record_count,
                records,
                records_sha256: input.records_sha256,
                mapping: Some(&input.map),
                records_offset: input.records_offset,
            })
        })
        .collect()
}

fn publish_streaming_compacted_run(
    sources: &[MergeSource<'_>],
    first_pass: MergeEvidence,
    epoch: u64,
    runs_dir: &Path,
    file_name: &str,
    options: PackStoreOptions,
) -> Result<IndexRun> {
    let final_path = runs_dir.join(file_name);
    let temp_path = runs_dir.join(format!("{file_name}.tmp"));
    ensure!(
        !final_path.exists(),
        "compacted output {} already exists and must be reclaimed before retry",
        final_path.display()
    );
    let fence_count = usize::try_from(first_pass.output_records.div_ceil(FENCE_INTERVAL as u64))
        .context("compacted fence count does not fit usize")?;
    let fence_capacity = fence_count
        .checked_mul(FENCE_KEY_BYTES)
        .context("compacted fence bytes overflow")?;
    let mut fences = Vec::with_capacity(fence_capacity);
    let mut bloom =
        BlockedBloomFilter::with_capacity(first_pass.output_records, filter_seed(epoch))?;
    let records_offset = u64::try_from(INDEX_HEADER_LEN)
        .context("index header length does not fit u64")?
        .checked_add(u64::try_from(fence_capacity).context("fence bytes do not fit u64")?)
        .and_then(|offset| {
            offset.checked_add(u64::try_from(bloom.as_bytes().len()).unwrap_or(u64::MAX))
        })
        .context("compacted records offset overflows")?;
    let file_bytes = records_offset
        .checked_add(
            first_pass
                .output_records
                .checked_mul(INDEX_RECORD_LEN as u64)
                .context("compacted record bytes overflow")?,
        )
        .context("compacted run length overflows")?;

    let file = OpenOptions::new()
        .create_new(true)
        .read(true)
        .write(true)
        .open(&temp_path)
        .with_context(|| format!("create compacted run {}", temp_path.display()))?;
    let mut temp_guard = TempRunGuard::new(temp_path.clone());
    file.set_len(file_bytes)
        .with_context(|| format!("size compacted run {}", temp_path.display()))?;
    let second_pass = {
        let mut writer = BufWriter::with_capacity(COMPACTION_IO_BUFFER_BYTES, &file);
        writer
            .seek(SeekFrom::Start(records_offset))
            .context("seek compacted record writer")?;
        let evidence = merge_sorted_runs(sources, |ordinal, entry, encoded| {
            if ordinal.is_multiple_of(FENCE_INTERVAL as u64) {
                fences.extend_from_slice(&entry.key[..FENCE_KEY_BYTES]);
            }
            bloom.insert_hash(key_hash(&entry.key));
            writer
                .write_all(encoded)
                .context("write compacted index record")
        })?;
        writer.flush().context("flush compacted index records")?;
        evidence
    };
    ensure!(
        second_pass == first_pass,
        "compaction pass evidence changed between count and write"
    );
    ensure!(
        fences.len() == fence_capacity,
        "compacted fence count changed during write"
    );

    let header = encode_compacted_index_header(epoch, first_pass, &fences, &bloom)?;
    {
        let mut writer = BufWriter::with_capacity(COMPACTION_IO_BUFFER_BYTES, &file);
        writer
            .seek(SeekFrom::Start(0))
            .context("seek compacted structure writer")?;
        writer
            .write_all(&header)
            .context("write compacted index header")?;
        writer
            .write_all(&fences)
            .context("write compacted index fences")?;
        writer
            .write_all(bloom.as_bytes())
            .context("write compacted Bloom filter")?;
        writer.flush().context("flush compacted index structure")?;
    }
    file.sync_data()
        .with_context(|| format!("sync compacted run {}", temp_path.display()))?;
    drop(fences);
    drop(bloom);
    verify_file_record_digest(
        &file,
        records_offset,
        first_pass.output_records,
        first_pass.records_sha256,
    )?;
    drop(file);
    fs::rename(&temp_path, &final_path).with_context(|| {
        format!(
            "publish compacted run {} as {}",
            temp_path.display(),
            final_path.display()
        )
    })?;
    temp_guard.disarm();
    sync_directory(runs_dir)?;
    match read_index_run_with_options(&final_path, options) {
        Ok(run) => {
            ensure!(
                run.format_version == PACK_INDEX_RUN_FORMAT_VERSION
                    && run.record_count == first_pass.output_records
                    && run.records_sha256 == first_pass.records_sha256,
                "published compacted run differs from its merge evidence"
            );
            Ok(run)
        }
        Err(error) => {
            let _ = fs::remove_file(&final_path);
            let _ = sync_directory(runs_dir);
            Err(error).context("validate published compacted run")
        }
    }
}

fn encode_compacted_index_header(
    epoch: u64,
    evidence: MergeEvidence,
    fences: &[u8],
    bloom: &BlockedBloomFilter,
) -> Result<[u8; INDEX_HEADER_LEN]> {
    ensure!(
        fences.len().is_multiple_of(FENCE_KEY_BYTES),
        "compacted fence section is misaligned"
    );
    let mut header = [0u8; INDEX_HEADER_LEN];
    header[0..8].copy_from_slice(INDEX_MAGIC);
    header[8..12].copy_from_slice(&PACK_INDEX_RUN_FORMAT_VERSION.to_le_bytes());
    header[12..16].copy_from_slice(&(INDEX_HEADER_LEN as u32).to_le_bytes());
    header[16..24].copy_from_slice(&epoch.to_le_bytes());
    header[24..32].copy_from_slice(&evidence.output_records.to_le_bytes());
    header[32..64].copy_from_slice(&evidence.records_sha256);
    header[64..68].copy_from_slice(
        &u32::try_from(fences.len() / FENCE_KEY_BYTES)
            .context("compacted fence count does not fit u32")?
            .to_le_bytes(),
    );
    header[68..72].copy_from_slice(&(FENCE_INTERVAL as u32).to_le_bytes());
    header[72..80].copy_from_slice(&bloom.seed().to_le_bytes());
    header[80..88].copy_from_slice(
        &u64::try_from(bloom.as_bytes().len())
            .context("Bloom byte length does not fit u64")?
            .to_le_bytes(),
    );
    header[88..121].copy_from_slice(&evidence.min_key);
    header[121..154].copy_from_slice(&evidence.max_key);
    header[INDEX_STRUCTURE_SHA256_END..INDEX_HEADER_TAG_START]
        .copy_from_slice(&(BLOOM_HASH_PROBES as u16).to_le_bytes());
    let structure_sha256 = index_structure_digest_parts(
        PACK_INDEX_RUN_FORMAT_VERSION,
        &header,
        fences,
        bloom.as_bytes(),
    )?;
    header[INDEX_STRUCTURE_SHA256_START..INDEX_STRUCTURE_SHA256_END]
        .copy_from_slice(&structure_sha256);
    let tag = digest(&header[..INDEX_HEADER_TAG_START]);
    header[INDEX_HEADER_TAG_START..INDEX_HEADER_LEN].copy_from_slice(&tag[..4]);
    Ok(header)
}

fn verify_file_record_digest(
    file: &File,
    records_offset: u64,
    record_count: u64,
    expected: [u8; 32],
) -> Result<()> {
    let mut remaining = record_count
        .checked_mul(INDEX_RECORD_LEN as u64)
        .context("compacted verification length overflows")?;
    let mut offset = records_offset;
    let mut buffer = vec![0u8; COMPACTION_IO_BUFFER_BYTES];
    let mut hasher = Sha256::new();
    while remaining > 0 {
        let bytes = usize::try_from(remaining.min(buffer.len() as u64))
            .context("verification chunk length does not fit usize")?;
        file.read_exact_at(&mut buffer[..bytes], offset)
            .context("read compacted records for verification")?;
        hasher.update(&buffer[..bytes]);
        offset = offset
            .checked_add(bytes as u64)
            .context("verification offset overflows")?;
        remaining -= bytes as u64;
    }
    ensure!(
        <[u8; 32]>::from(hasher.finalize()) == expected,
        "persisted compacted record checksum mismatch"
    );
    Ok(())
}

struct TempRunGuard {
    path: PathBuf,
    armed: bool,
}

impl TempRunGuard {
    fn new(path: PathBuf) -> Self {
        Self { path, armed: true }
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for TempRunGuard {
    fn drop(&mut self) {
        if self.armed {
            let _ = fs::remove_file(&self.path);
        }
    }
}
