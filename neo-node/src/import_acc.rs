//! Offline `.acc` block importer.
//!
//! The file format is:
//! - `u32` start index (little-endian)
//! - `u32` block count (little-endian)
//! - repeated `count` times:
//!   - `u32` block byte length
//!   - serialized `Block` payload bytes

use anyhow::{Context, Result, bail};
use neo_core::{
    neo_io::{MemoryReader, Serializable},
    neo_system::NeoSystem,
    network::p2p::payloads::block::Block,
    persistence::{StorageConfig, StoreCache, StoreProvider, providers::RocksDBStoreProvider},
    smart_contract::native::LedgerContract,
};
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
    time::Instant,
};
use tracing::info;

#[derive(Debug, Clone, Copy)]
pub struct ImportSummary {
    pub declared_start: u32,
    pub declared_count: u32,
    pub imported: u64,
    pub skipped: u64,
    pub final_height: u32,
    pub elapsed_secs: f64,
}

const ACC_INDEX_MAGIC: &[u8; 8] = b"NEOACCIX";
const ACC_INDEX_HEADER_LEN: u64 = 32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct IndexedResumePosition {
    first_item: u32,
    skipped: u64,
    offset: u64,
}

fn indexed_resume_position(
    acc_path: &Path,
    declared_start: u32,
    declared_count: u32,
    local_height: u32,
) -> Result<Option<IndexedResumePosition>> {
    let Some(resume_height) = local_height.checked_add(1) else {
        return Ok(None);
    };
    let Some(declared_end) = declared_start.checked_add(declared_count) else {
        bail!("declared ACC range overflow");
    };
    if resume_height < declared_start || resume_height >= declared_end {
        return Ok(None);
    }

    let index_path = acc_path.with_file_name(format!(
        "{}.offsets",
        acc_path
            .file_name()
            .and_then(|name| name.to_str())
            .context("ACC path has no UTF-8 file name")?
    ));
    if !index_path.is_file() {
        return Ok(None);
    }

    let acc_metadata = acc_path.metadata().context("failed to stat ACC file")?;
    let modified_ns = acc_metadata
        .modified()
        .context("failed to read ACC modification time")?
        .duration_since(std::time::UNIX_EPOCH)
        .context("ACC modification time precedes Unix epoch")?
        .as_nanos();
    let modified_ns = u64::try_from(modified_ns).context("ACC modification time overflow")?;
    let index_metadata = index_path
        .metadata()
        .context("failed to stat ACC offset index")?;
    let expected_index_len = ACC_INDEX_HEADER_LEN
        .checked_add(u64::from(declared_count) * 8)
        .context("ACC offset index length overflow")?;
    if index_metadata.len() != expected_index_len {
        bail!(
            "invalid ACC offset index length: expected {}, got {}",
            expected_index_len,
            index_metadata.len()
        );
    }

    let mut index = File::open(&index_path).context("failed to open ACC offset index")?;
    let mut header = [0u8; ACC_INDEX_HEADER_LEN as usize];
    index
        .read_exact(&mut header)
        .context("failed to read ACC offset index header")?;
    let read_u32 = |offset: usize| {
        u32::from_le_bytes(
            header[offset..offset + 4]
                .try_into()
                .expect("four-byte field"),
        )
    };
    let read_u64 = |offset: usize| {
        u64::from_le_bytes(
            header[offset..offset + 8]
                .try_into()
                .expect("eight-byte field"),
        )
    };
    if &header[..8] != ACC_INDEX_MAGIC
        || read_u32(8) != declared_start
        || read_u32(12) != declared_count
        || read_u64(16) != acc_metadata.len()
        || read_u64(24) != modified_ns
    {
        bail!(
            "stale or invalid ACC offset index for {}; delete it to rebuild",
            acc_path.display()
        );
    }

    let first_item = resume_height - declared_start;
    index
        .seek(SeekFrom::Start(
            ACC_INDEX_HEADER_LEN + u64::from(first_item) * 8,
        ))
        .context("failed to seek within ACC offset index")?;
    let mut offset_bytes = [0u8; 8];
    index
        .read_exact(&mut offset_bytes)
        .context("failed to read ACC block offset")?;
    let offset = u64::from_le_bytes(offset_bytes);
    if offset < 8 || offset.saturating_add(4) > acc_metadata.len() {
        bail!("ACC offset index contains an out-of-range block offset");
    }

    Ok(Some(IndexedResumePosition {
        first_item,
        skipped: u64::from(first_item),
        offset,
    }))
}

fn import_stop_height_from_env() -> Option<u32> {
    std::env::var("NEO_IMPORT_STOP_HEIGHT")
        .ok()
        .and_then(|raw| raw.trim().parse::<u32>().ok())
}

fn import_progress_interval_from_env() -> u64 {
    std::env::var("NEO_IMPORT_PROGRESS_INTERVAL")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(1_000)
}

fn import_progress_heartbeat_secs_from_env() -> u64 {
    std::env::var("NEO_IMPORT_PROGRESS_HEARTBEAT_SECS")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(60)
}

fn import_flush_interval_from_env() -> u64 {
    std::env::var("NEO_IMPORT_FLUSH_INTERVAL")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(10_000)
}

fn read_local_view_height(system: &Arc<NeoSystem>) -> Option<u32> {
    let mut current_block_key = Vec::with_capacity(5);
    current_block_key.extend_from_slice(&(-4i32).to_le_bytes());
    current_block_key.push(12u8);
    system
        .store()
        .snapshot()
        .as_ref()
        .try_get(&current_block_key)
        .and_then(|bytes| {
            bytes.get(32..36).map(|slice| {
                let mut index_bytes = [0u8; 4];
                index_bytes.copy_from_slice(slice);
                u32::from_le_bytes(index_bytes)
            })
        })
}

fn read_disk_view_height_and_hash(
    storage_path: Option<&str>,
    expected_height: u32,
) -> Result<Option<(u32, bool)>> {
    let Some(path) = storage_path else {
        return Ok(None);
    };

    let config = StorageConfig {
        path: path.into(),
        read_only: true,
        ..Default::default()
    };
    let provider = RocksDBStoreProvider::new(config);
    let store = provider
        .get_store("")
        .map_err(|err| anyhow::anyhow!(err.to_string()))
        .with_context(|| format!("failed to open read-only store at {}", path))?;

    let mut current_block_key = Vec::with_capacity(5);
    current_block_key.extend_from_slice(&(-4i32).to_le_bytes());
    current_block_key.push(12u8);
    let persisted_height = store
        .snapshot()
        .as_ref()
        .try_get(&current_block_key)
        .and_then(|bytes| {
            bytes.get(32..36).map(|slice| {
                let mut index_bytes = [0u8; 4];
                index_bytes.copy_from_slice(slice);
                u32::from_le_bytes(index_bytes)
            })
        })
        .unwrap_or(0);

    let snapshot = store.snapshot();
    let cache = StoreCache::new_from_snapshot(snapshot);
    let current_hash_written = LedgerContract::new()
        .get_block_hash_by_index(&cache, expected_height)
        .map_err(|err| anyhow::anyhow!(err.to_string()))?
        .is_some();

    Ok(Some((persisted_height, current_hash_written)))
}

fn flush_and_verify_checkpoint(
    system: &Arc<NeoSystem>,
    storage_path: Option<&str>,
    expected_height: u32,
    imported: u64,
    skipped: u64,
    reason: &str,
) -> Result<u32> {
    let started_at = Instant::now();
    system.store().flush();

    // The state service persists its roots in a *separate* store from the chain
    // store. Under the high-throughput import profile the write-ahead log is
    // disabled, so the state store's memtable must be flushed explicitly —
    // otherwise every computed root is lost when the process exits and
    // `getstateroot` returns null after a restart. Flush it here, then fail
    // closed below if the local root index has not reached the expected height.
    let state_store = system.state_store().ok().flatten();
    if let Some(state_store) = state_store.as_ref() {
        state_store.flush();
    }
    // Application logs live in their own store. The import profile disables the
    // write-ahead log, so leave them unflushed and a restart has no behaviour oracle.
    if let Ok(Some(logs)) =
        system.get_service::<neo_core::application_logs::ApplicationLogsService>()
    {
        logs.flush();
    }
    if let Ok(Some(tracker)) =
        system.get_service::<neo_core::tokens_tracker::TokensTrackerService>()
    {
        tracker.flush();
    }
    let state_root_index = state_store
        .as_ref()
        .map(|store| store.local_root_index().unwrap_or(0));

    let local_view_height = read_local_view_height(system).unwrap_or(0);

    let (verification_source, persisted_height, current_hash_written) =
        if let Some((disk_height, disk_hash_written)) =
            read_disk_view_height_and_hash(storage_path, expected_height)?
        {
            ("disk", disk_height, disk_hash_written)
        } else {
            let snapshot = system.store().snapshot();
            let cache = StoreCache::new_from_snapshot(snapshot);
            let hash_written = LedgerContract::new()
                .get_block_hash_by_index(&cache, expected_height)
                .map_err(|err| anyhow::anyhow!(err.to_string()))?
                .is_some();
            ("local", local_view_height, hash_written)
        };

    if persisted_height < expected_height {
        bail!(
            "flush verification failed ({reason}): expected persisted height >= {}, got {} ({verification_source} view)",
            expected_height,
            persisted_height
        );
    }

    if !current_hash_written {
        bail!(
            "flush verification failed ({reason}): block hash for height {} is missing after flush",
            expected_height
        );
    }

    // Fail closed: when the state service is enabled, its durable local root
    // index must have reached the imported height, otherwise the computed roots
    // would be silently unservable after a restart.
    if let Some(state_root_index) = state_root_index
        && state_root_index < expected_height
    {
        bail!(
            "flush verification failed ({reason}): state service local root index {} is behind expected height {}",
            state_root_index,
            expected_height
        );
    }

    info!(
        target: "neo",
        reason,
        imported,
        skipped,
        expected_height,
        persisted_height,
        local_view_height,
        state_root_index = ?state_root_index,
        verification_source,
        flush_elapsed_ms = started_at.elapsed().as_millis(),
        "acc import storage checkpoint flush completed"
    );

    Ok(persisted_height)
}

pub fn import_acc_file(
    system: &Arc<NeoSystem>,
    path: &Path,
    storage_path: Option<&str>,
) -> Result<ImportSummary> {
    if !path.exists() {
        bail!("import file does not exist: {}", path.display());
    }
    if path
        .extension()
        .and_then(|ext| ext.to_str())
        .is_some_and(|ext| ext.eq_ignore_ascii_case("zip"))
    {
        bail!(
            "zip input is not supported directly; unzip first and pass the .acc file path: {}",
            path.display()
        );
    }

    let file = File::open(path)
        .with_context(|| format!("failed to open import file {}", path.display()))?;
    let mut reader = BufReader::new(file);

    let mut header = [0u8; 8];
    reader
        .read_exact(&mut header)
        .with_context(|| format!("failed to read import header from {}", path.display()))?;
    let declared_start = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let declared_count = u32::from_le_bytes([header[4], header[5], header[6], header[7]]);

    let mut current_height = system.current_block_index();
    let started_at = Instant::now();
    let mut imported = 0u64;
    let mut skipped = 0u64;
    let mut size_buf = [0u8; 4];
    let mut payload = Vec::<u8>::new();
    let stop_height = import_stop_height_from_env();
    let progress_interval = import_progress_interval_from_env();
    let progress_heartbeat_secs = import_progress_heartbeat_secs_from_env();
    let flush_interval = import_flush_interval_from_env();
    let mut last_progress_log_at = Instant::now();

    // Import is an offline bootstrap path: keep fast-sync mode enabled to skip
    // expensive event fan-out while preserving full block execution/validation.
    let was_fast_sync = system.context().is_fast_sync_mode();
    if !was_fast_sync {
        system.context().enable_fast_sync_mode();
    }
    system.store().enable_fast_sync_mode();

    info!(
        target: "neo",
        file = %path.display(),
        declared_start,
        declared_count,
        local_height = current_height,
        stop_height = ?stop_height,
        progress_interval,
        progress_heartbeat_secs,
        flush_interval,
        "starting .acc import"
    );

    let first_item = if let Some(resume) =
        indexed_resume_position(path, declared_start, declared_count, current_height)?
    {
        reader
            .seek(SeekFrom::Start(resume.offset))
            .context("failed to seek ACC import to the persisted height")?;
        skipped = resume.skipped;
        info!(
            target: "neo",
            resume_height = current_height.saturating_add(1),
            first_item = resume.first_item,
            skipped,
            offset = resume.offset,
            seek_elapsed_ms = started_at.elapsed().as_millis(),
            "acc import offset seek completed"
        );
        resume.first_item
    } else {
        0
    };

    let result: Result<ImportSummary> = (|| {
        for i in first_item..declared_count {
            reader
                .read_exact(&mut size_buf)
                .with_context(|| format!("failed to read block size at item {}", i))?;
            let block_size = u32::from_le_bytes(size_buf) as usize;
            if block_size == 0 {
                bail!("encountered zero-sized block payload at item {}", i);
            }

            let Some(declared_index) = declared_start.checked_add(i) else {
                bail!("declared block index overflow at item {}", i);
            };

            if declared_index <= current_height {
                let skip_len = i64::try_from(block_size)
                    .with_context(|| format!("block size too large to skip at item {}", i))?;
                reader
                    .seek(SeekFrom::Current(skip_len))
                    .with_context(|| format!("failed to seek past block payload at item {}", i))?;
                skipped += 1;
                if skipped.is_multiple_of(100_000) {
                    info!(
                        target: "neo",
                        skipped,
                        current_height,
                        "acc import scan progress"
                    );
                }
                continue;
            }

            payload.resize(block_size, 0u8);
            reader
                .read_exact(&mut payload[..block_size])
                .with_context(|| format!("failed to read block payload at item {}", i))?;

            let mut block_reader = MemoryReader::new(&payload[..block_size]);
            let block = <Block as Serializable>::deserialize(&mut block_reader)
                .with_context(|| format!("failed to deserialize block payload at item {}", i))?;
            let block_index = block.index();

            if block_index != declared_index {
                bail!(
                    "import block index mismatch at item {}: declared {}, decoded {}",
                    i,
                    declared_index,
                    block_index
                );
            }

            let expected = current_height.saturating_add(1);
            if block_index != expected {
                bail!(
                    "import sequence gap at item {}: expected block {}, got {}",
                    i,
                    expected,
                    block_index
                );
            }

            system
                .persist_block_without_runtime_cache(block)
                .with_context(|| format!("failed to persist imported block {}", block_index))?;
            current_height = block_index;
            imported += 1;

            let periodic_flush_due = imported.is_multiple_of(flush_interval);
            if periodic_flush_due {
                let _ = flush_and_verify_checkpoint(
                    system,
                    storage_path,
                    current_height,
                    imported,
                    skipped,
                    "periodic",
                )?;
            }

            if stop_height.is_some_and(|limit| current_height >= limit) {
                if !periodic_flush_due {
                    let _ = flush_and_verify_checkpoint(
                        system,
                        storage_path,
                        current_height,
                        imported,
                        skipped,
                        "stop-height",
                    )?;
                }
                info!(
                    target: "neo",
                    imported,
                    current_height,
                    stop_height = ?stop_height,
                    "reached NEO_IMPORT_STOP_HEIGHT; ending import early"
                );
                break;
            }

            let should_log_progress = imported.is_multiple_of(progress_interval)
                || last_progress_log_at.elapsed().as_secs() >= progress_heartbeat_secs;
            if should_log_progress {
                let elapsed = started_at.elapsed().as_secs_f64();
                let rate = if elapsed > 0.0 {
                    imported as f64 / elapsed
                } else {
                    0.0
                };
                let local_view_height = read_local_view_height(system).unwrap_or(0);
                let current_hash_written = LedgerContract::new()
                    .get_block_hash_by_index(&system.context().store_cache(), current_height)
                    .ok()
                    .flatten()
                    .is_some();
                info!(
                    target: "neo",
                    imported,
                    skipped,
                    current_height,
                    local_view_height,
                    current_hash_written,
                    rate_blocks_per_sec = rate,
                    "acc import progress"
                );
                last_progress_log_at = Instant::now();
            }
        }

        let elapsed_secs = started_at.elapsed().as_secs_f64();
        let rate = if elapsed_secs > 0.0 {
            imported as f64 / elapsed_secs
        } else {
            0.0
        };

        info!(
            target: "neo",
            file = %path.display(),
            declared_start,
            declared_count,
            imported,
            skipped,
            final_height = current_height,
            elapsed_secs,
            rate_blocks_per_sec = rate,
            "completed .acc import"
        );

        Ok(ImportSummary {
            declared_start,
            declared_count,
            imported,
            skipped,
            final_height: current_height,
            elapsed_secs,
        })
    })();

    system.store().disable_fast_sync_mode();
    if !was_fast_sync {
        system.context().disable_fast_sync_mode();
    }

    match result {
        Ok(summary) => {
            let _ = flush_and_verify_checkpoint(
                system,
                storage_path,
                summary.final_height,
                summary.imported,
                summary.skipped,
                "final",
            )?;
            Ok(summary)
        }
        Err(err) => {
            system.store().flush();
            // Preserve whatever state roots were computed before the failure so a
            // restart can still serve them (the state store is a separate store
            // and is not covered by `system.store().flush()`).
            if let Ok(Some(state_store)) = system.state_store() {
                state_store.flush();
            }
            if let Ok(Some(logs)) =
                system.get_service::<neo_core::application_logs::ApplicationLogsService>()
            {
                logs.flush();
            }
            if let Ok(Some(tracker)) =
                system.get_service::<neo_core::tokens_tracker::TokensTrackerService>()
            {
                tracker.flush();
            }
            info!(
                target: "neo",
                "storage flush completed after .acc import error"
            );
            Err(err)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_acc_and_index() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
        let temp = tempfile::tempdir().expect("temporary directory");
        let acc_path = temp.path().join("chain.0.acc");
        let index_path = temp.path().join("chain.0.acc.offsets");
        let payloads: &[&[u8]] = &[&[0x10, 0x11], &[0x20, 0x21, 0x22], &[0x30]];

        let mut offsets = Vec::new();
        {
            let mut acc = File::create(&acc_path).expect("create ACC");
            acc.write_all(&10u32.to_le_bytes()).expect("ACC start");
            acc.write_all(&3u32.to_le_bytes()).expect("ACC count");
            for payload in payloads {
                offsets.push(acc.stream_position().expect("ACC offset"));
                acc.write_all(&(payload.len() as u32).to_le_bytes())
                    .expect("block length");
                acc.write_all(payload).expect("block payload");
            }
        }

        let metadata = acc_path.metadata().expect("ACC metadata");
        let modified_ns = metadata
            .modified()
            .expect("ACC modification time")
            .duration_since(std::time::UNIX_EPOCH)
            .expect("post-epoch timestamp")
            .as_nanos() as u64;
        {
            let mut index = File::create(&index_path).expect("create index");
            index.write_all(ACC_INDEX_MAGIC).expect("index magic");
            index.write_all(&10u32.to_le_bytes()).expect("index start");
            index.write_all(&3u32.to_le_bytes()).expect("index count");
            index
                .write_all(&metadata.len().to_le_bytes())
                .expect("index ACC size");
            index
                .write_all(&modified_ns.to_le_bytes())
                .expect("index ACC modification time");
            for offset in offsets {
                index
                    .write_all(&offset.to_le_bytes())
                    .expect("block offset");
            }
        }

        (temp, acc_path, index_path)
    }

    #[test]
    fn offset_index_resumes_at_first_unpersisted_block() {
        let (_temp, acc_path, _index_path) = write_acc_and_index();
        let resume = indexed_resume_position(&acc_path, 10, 3, 11)
            .expect("valid offset index")
            .expect("indexed resume");

        assert_eq!(resume.first_item, 2);
        assert_eq!(resume.skipped, 2);
        let mut acc = File::open(&acc_path).expect("open ACC");
        acc.seek(SeekFrom::Start(resume.offset))
            .expect("seek to resume block");
        let mut size = [0u8; 4];
        acc.read_exact(&mut size).expect("resume block size");
        assert_eq!(u32::from_le_bytes(size), 1);
    }

    #[test]
    fn stale_offset_index_is_rejected() {
        let (_temp, acc_path, index_path) = write_acc_and_index();
        let mut index = std::fs::OpenOptions::new()
            .write(true)
            .open(&index_path)
            .expect("open index");
        index
            .seek(SeekFrom::Start(8))
            .and_then(|_| index.write_all(&99u32.to_le_bytes()))
            .expect("corrupt index start");

        let error = indexed_resume_position(&acc_path, 10, 3, 11)
            .expect_err("stale index must not be trusted");
        assert!(error.to_string().contains("stale or invalid"));
    }
}
