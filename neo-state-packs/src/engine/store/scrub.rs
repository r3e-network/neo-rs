use super::*;

pub(super) fn scrub_live_index_run(
    live: &LiveRun,
    runs_dir: &Path,
    committed_pack_bytes: u64,
) -> Result<()> {
    let path = runs_dir.join(run_file_name(live.level, live.min_epoch, live.max_epoch));
    let file = File::open(&path)
        .with_context(|| format!("open index run {} for scrub", path.display()))?;
    let file_bytes = file
        .metadata()
        .with_context(|| format!("stat index run {} for scrub", path.display()))?
        .len();
    ensure!(
        file_bytes == live.run.file_bytes,
        "index run length changed during scrub"
    );
    let map = Mmap::map_sequential(&file, file_bytes, &path)?;
    let records_start =
        usize::try_from(live.run.records_offset).context("scrub records offset overflows")?;
    let records_len = usize::try_from(live.run.record_count)
        .context("scrub record count does not fit usize")?
        .checked_mul(INDEX_RECORD_LEN)
        .context("scrub record byte length overflows")?;
    let records_end = records_start
        .checked_add(records_len)
        .context("scrub record range overflows")?;
    let records = map
        .as_slice()
        .get(records_start..records_end)
        .context("scrub record range lies outside the run")?;
    let records_per_chunk = (1024 * 1024 / INDEX_RECORD_LEN).max(1);
    let chunk_bytes = records_per_chunk * INDEX_RECORD_LEN;
    let mut hasher = Sha256::new();
    let mut previous: Option<IndexEntry> = None;
    let mut ordinal = 0usize;
    let mut release_start = records_start;
    for chunk in records.chunks(chunk_bytes) {
        hasher.update(chunk);
        for record in chunk.chunks_exact(INDEX_RECORD_LEN) {
            let entry = decode_record(record)?;
            validate_index_entry_payload_range(&entry, committed_pack_bytes)?;
            if let Some(previous) = previous {
                ensure!(
                    previous.key < entry.key
                        || (previous.key == entry.key && previous.sequence < entry.sequence),
                    "index run records are not ordered by key and sequence"
                );
            }
            if ordinal.is_multiple_of(FENCE_INTERVAL) {
                ensure!(
                    live.run.fences.get(ordinal / FENCE_INTERVAL)
                        == Some(&truncate_key(&entry.key)),
                    "index fence does not match its record block"
                );
            }
            ensure!(
                live.run
                    .filter
                    .maybe_contains_hash(&map, key_hash(&entry.key))?,
                "index filter rejected a persisted key"
            );
            previous = Some(entry);
            ordinal += 1;
        }
        let absolute_end = records_start
            .checked_add(ordinal * INDEX_RECORD_LEN)
            .context("scrub reclaim range overflows")?;
        release_start = map.advise_dontneed(release_start, absolute_end)?;
    }
    ensure!(
        ordinal == usize::try_from(live.run.record_count).unwrap_or(usize::MAX),
        "index scrub record count mismatch"
    );
    ensure!(
        <[u8; 32]>::from(hasher.finalize()) == live.run.records_sha256,
        "index records checksum mismatch during scrub"
    );
    let _ = map.advise_dontneed(release_start, map.as_slice().len())?;
    Ok(())
}

pub(super) fn validate_index_entry_payload_range(
    entry: &IndexEntry,
    committed_pack_bytes: u64,
) -> Result<()> {
    if entry.tombstone {
        ensure!(
            entry.value_len == 0,
            "tombstone index entry carries a value"
        );
        return Ok(());
    }
    let value_end = entry
        .value_offset
        .checked_add(u64::from(entry.value_len))
        .context("indexed value range overflows")?;
    ensure!(
        value_end <= committed_pack_bytes,
        "indexed value range ends at {value_end}, beyond the committed pack length of {committed_pack_bytes} bytes"
    );
    Ok(())
}
