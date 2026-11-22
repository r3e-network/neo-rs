use anyhow::{Context, Result, bail};
use neo_native_contracts::native::LedgerContract;
use neo_primitives::UInt256;
use neo_rpc::application_logs::ApplicationLogsService;
use neo_storage::persistence::providers::RocksDBStoreProvider;
use neo_storage::persistence::{StoreCache, StoreProvider, storage::StorageConfig};
use neo_system::NeoSystem;
use std::{sync::Arc, time::Instant};
use tracing::info;

/// The persisted current-block record: the height a restart would resume from
/// together with the block hash recorded for that height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct PersistedCheckpoint {
    pub(super) height: u32,
    pub(super) hash: UInt256,
}

fn read_persisted_checkpoint(cache: &StoreCache) -> PersistedCheckpoint {
    let ledger = LedgerContract::new();
    PersistedCheckpoint {
        height: ledger.current_index(cache).unwrap_or(0),
        hash: ledger.current_hash(cache).unwrap_or_default(),
    }
}

/// Reads the checkpoint from the current in-process store snapshot.
pub(super) fn read_local_checkpoint(system: &Arc<NeoSystem>) -> PersistedCheckpoint {
    let snapshot = system.store().snapshot();
    let cache = StoreCache::new_from_snapshot(snapshot);
    read_persisted_checkpoint(&cache)
}

/// Reopens the configured storage path read-only and reads the checkpoint a
/// fresh process would see, instead of trusting in-process state.
fn read_disk_checkpoint(storage_path: Option<&str>) -> Result<Option<PersistedCheckpoint>> {
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
        .with_context(|| format!("failed to open read-only store at {path}"))?;

    let cache = StoreCache::new_from_snapshot(store.snapshot());
    Ok(Some(read_persisted_checkpoint(&cache)))
}

/// Requires the persisted current block to be exactly the last accepted block.
///
/// Presence is not enough: a checkpoint that still points at a different hash
/// (or at another height) must fail closed, because the very next import would
/// otherwise build on a tip this run never proved durable.
pub(super) fn verify_persisted_checkpoint(
    reason: &str,
    verification_source: &str,
    expected_height: u32,
    expected_hash: &UInt256,
    persisted: PersistedCheckpoint,
) -> Result<()> {
    if persisted.height != expected_height {
        bail!(
            "checkpoint verification failed ({reason}, {verification_source} view): persisted current height {} does not match the last accepted block height {expected_height}",
            persisted.height
        );
    }
    if persisted.hash != *expected_hash {
        bail!(
            "checkpoint verification failed ({reason}, {verification_source} view): persisted current block hash {} at height {expected_height} does not match the last accepted block hash {expected_hash}",
            persisted.hash
        );
    }
    Ok(())
}

/// Requires the durable logs watermark to agree with the checkpoint it covers.
///
/// The checkpoint order makes the chain durable first and the logs second, so the
/// two databases can only ever be wrong in one direction: the logs may trail the
/// chain after a crash between the two flushes, and they may never run ahead of
/// it. Records committed by this checkpoint cover every block up to
/// `expected_height`, so the watermark must name exactly that block; a checkpoint
/// that committed nothing (an already verified height) only requires that the
/// logs do not claim a block the chain has not made durable.
pub(super) fn verify_logs_watermark(
    logs: &ApplicationLogsService,
    expected_height: u32,
    expected_hash: &UInt256,
    committed_records: u64,
    reason: &str,
) -> Result<()> {
    let Some((height, hash)) = logs.watermark() else {
        // Nothing committed means nothing to advance; this is also what a plugin
        // that stopped on its exception policy looks like, which must not fail
        // the import.
        if committed_records == 0 {
            return Ok(());
        }
        bail!(
            "flush verification failed ({reason}): {committed_records} application log records were committed without a durable block watermark"
        );
    };
    if height > expected_height {
        bail!(
            "flush verification failed ({reason}): application logs watermark height {height} is ahead of the checkpoint height {expected_height}"
        );
    }
    if committed_records > 0 && (height != expected_height || hash != *expected_hash) {
        bail!(
            "flush verification failed ({reason}): application logs watermark {height}/{hash} does not match the checkpoint {expected_height}/{expected_hash}"
        );
    }
    Ok(())
}

/// Commits, durably flushes and verifies the application logs of one checkpoint.
///
/// The three steps are ordered, and each one gates the next:
///
/// 1. `flush_accepted` writes the records the session accumulated into the logs
///    database but advances no watermark.
/// 2. `try_flush_durable` forces that database to disk and only then promotes the
///    watermark the commit covered. A backend failure returns here and leaves the
///    watermark where it was.
/// 3. `verify_logs_watermark` compares the now-promoted watermark with the chain
///    height of this checkpoint.
///
/// The caller runs this after the chain's own fallible flush, so the cross-store
/// order is chain, logs commit, logs durability, watermark, and only then a
/// checkpoint that may be reported successful.
fn checkpoint_application_logs(
    logs: &ApplicationLogsService,
    expected_height: u32,
    expected_hash: &UInt256,
    reason: &str,
) -> Result<(u64, u64)> {
    let flushed = logs
        .flush_accepted()
        .map_err(|err| anyhow::anyhow!(err))
        .with_context(|| format!("application logs flush failed ({reason})"))?;
    // Fail closed: `flush_accepted` only put the bytes in the backend under a
    // write-ahead-log-less profile, so an unconfirmed flush must never reach the
    // watermark comparison below.
    logs.try_flush_durable()
        .map_err(|err| anyhow::anyhow!(err))
        .with_context(|| format!("durable application-logs flush failed ({reason})"))?;
    verify_logs_watermark(
        logs,
        expected_height,
        expected_hash,
        flushed.records,
        reason,
    )?;
    Ok((flushed.records, flushed.bytes))
}

pub(super) fn flush_and_verify_checkpoint(
    system: &Arc<NeoSystem>,
    storage_path: Option<&str>,
    expected_height: u32,
    expected_hash: &UInt256,
    imported: u64,
    skipped: u64,
    reason: &str,
) -> Result<PersistedCheckpoint> {
    let started_at = Instant::now();

    // Fail closed: a checkpoint is only reported successful after a *fallible*
    // durable flush. `flush()` swallows backend errors, so it cannot back a
    // success report under the WAL-less import profile.
    system
        .store()
        .try_flush_durable()
        .map_err(|err| anyhow::anyhow!(err.to_string()))
        .with_context(|| format!("durable chain-store flush failed ({reason})"))?;

    // State roots are staged into the same backend store instance by this
    // assembly, so the durable flush above already covers their memtable. The
    // state-store port exposes no fallible flush, so this call stays best effort
    // and the fail-closed guarantee below is the local root index check.
    let state_store = system.state_store().ok().flatten();
    if let Some(state_store) = state_store.as_ref() {
        state_store.flush();
    }
    // Application logs live in their own store. The import profile disables the
    // write-ahead log, so leave them unflushed and a restart has no behaviour oracle.
    //
    // Cross-store order is the whole durability contract here, because the two
    // databases cannot be committed atomically: the chain is durable first (the
    // fallible flush above), then the records the session accumulated are
    // committed, then their store is flushed, and only then is the watermark
    // compared with the chain. A crash anywhere in between leaves the logs behind
    // the chain, never ahead of it.
    let mut application_log_records = 0u64;
    let mut application_log_bytes = 0u64;
    if let Ok(Some(logs)) = system.get_service::<ApplicationLogsService>() {
        let (records, bytes) =
            checkpoint_application_logs(logs.as_ref(), expected_height, expected_hash, reason)?;
        application_log_records = records;
        application_log_bytes = bytes;
    }
    if let Ok(Some(tracker)) = system.get_service::<neo_rpc::tokens_tracker::TokensTrackerService>()
    {
        // The token tracker does expose its backend, so its checkpoint is
        // durable rather than best effort.
        tracker
            .store()
            .try_flush_durable()
            .map_err(|err| anyhow::anyhow!(err.to_string()))
            .with_context(|| format!("durable token-tracker flush failed ({reason})"))?;
    }
    let state_root_index = state_store.as_ref().map(|store| store.local_root_index());

    let local_checkpoint = read_local_checkpoint(system);

    let (verification_source, persisted) = match read_disk_checkpoint(storage_path)? {
        Some(disk) => ("disk", disk),
        None => ("local", local_checkpoint),
    };
    verify_persisted_checkpoint(
        reason,
        verification_source,
        expected_height,
        expected_hash,
        persisted,
    )?;

    // Fail closed: when the state service is enabled, its durable local root
    // index must have reached the imported height, otherwise the computed roots
    // would be silently unservable after a restart.
    if let Some(state_root_index) = state_root_index
        && state_root_index < u64::from(expected_height)
    {
        bail!(
            "flush verification failed ({reason}): state service local root index {state_root_index} is behind expected height {expected_height}"
        );
    }

    info!(
        target: "neo",
        reason,
        imported,
        skipped,
        expected_height,
        expected_hash = %expected_hash,
        persisted_height = persisted.height,
        persisted_hash = %persisted.hash,
        local_view_height = local_checkpoint.height,
        state_root_index = ?state_root_index,
        verification_source,
        application_log_records,
        application_log_bytes,
        flush_elapsed_ms = started_at.elapsed().as_millis(),
        "acc import storage checkpoint flush completed"
    );

    Ok(persisted)
}
