//! Offline `.acc` block importer.
//!
//! The file format is:
//! - `u32` start index (little-endian)
//! - `u32` block count (little-endian)
//! - repeated `count` times:
//!   - `u32` block byte length
//!   - serialized `Block` payload bytes

use anyhow::{bail, Context, Result};
use neo_core::{
    neo_io::{MemoryReader, Serializable},
    neo_system::NeoSystem,
    network::p2p::payloads::block::Block,
    persistence::{providers::RocksDBStoreProvider, IStoreProvider, StorageConfig, StoreCache},
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
        .get_snapshot()
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
        .get_snapshot()
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

    let snapshot = store.get_snapshot();
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
    let local_view_height = read_local_view_height(system).unwrap_or(0);

    let (verification_source, persisted_height, current_hash_written) =
        if let Some((disk_height, disk_hash_written)) =
            read_disk_view_height_and_hash(storage_path, expected_height)?
        {
            ("disk", disk_height, disk_hash_written)
        } else {
            let snapshot = system.store().get_snapshot();
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

    info!(
        target: "neo",
        reason,
        imported,
        skipped,
        expected_height,
        persisted_height,
        local_view_height,
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

    let result: Result<ImportSummary> = (|| {
        for i in 0..declared_count {
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
                if skipped % 100_000 == 0 {
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
                .persist_block(block)
                .with_context(|| format!("failed to persist imported block {}", block_index))?;
            current_height = block_index;
            imported += 1;

            let periodic_flush_due = imported % flush_interval == 0;
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

            let should_log_progress = imported % progress_interval == 0
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
            info!(
                target: "neo",
                "storage flush completed after .acc import error"
            );
            Err(err)
        }
    }
}
