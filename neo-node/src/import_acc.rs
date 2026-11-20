//! Offline `.acc` block importer.
//!
//! The file format is:
//! - `u32` start index (little-endian)
//! - `u32` block count (little-endian)
//! - repeated `count` times:
//!   - `u32` block byte length
//!   - serialized `Block` payload bytes

use crate::import_batching::{
    BlockMeta, DrainTrigger, ImportDrainPolicy, MAX_EMPTY_BATCH_SIZE, clamp_empty_batch_size,
    empty_run_drain_trigger, validate_empty_run,
};
use crate::import_telemetry::{
    FastPathDeltas, ImportCounters, LogsDeltas, ParallelDeltas, PersistenceProfileDeltas,
    nanos_to_millis,
};
use anyhow::{Context, Result, bail};
use neo_io::{MemoryReader, Serializable};
use neo_native_contracts::native::LedgerContract;
use neo_primitives::UInt256;
use neo_protocol::payloads::block::Block;
use neo_rpc::application_logs::{
    ApplicationLogsService, DEFAULT_ACCEPTED_BYTE_CAP, ImportSessionOutcome, PersistenceUnitOutcome,
};
use neo_storage::persistence::providers::RocksDBStoreProvider;
use neo_storage::persistence::{StoreCache, StoreProvider, storage::StorageConfig};
use neo_system::{NeoSystem, PERSISTENCE_STAGE_NESTING};
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
    time::Instant,
};
use tracing::{info, warn};

#[derive(Debug, Clone, Copy)]
pub struct ImportSummary {
    pub declared_start: u32,
    pub declared_count: u32,
    pub imported: u64,
    pub skipped: u64,
    pub final_height: u32,
    pub elapsed_secs: f64,
    /// Run-cumulative block/transaction counters. Skipped archive records are
    /// never counted here.
    pub counters: ImportCounters,
    /// Fast-path counter delta measured across this run.
    pub fast_path: FastPathDeltas,
    /// Speculative-scheduler counter delta measured across this run.
    pub parallel: ParallelDeltas,
    /// Opt-in persistence-profile delta measured across this run.
    pub profile: PersistenceProfileDeltas,
    /// ApplicationLogs write coalescing measured across this run.
    pub logs: LogsDeltas,
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
            "invalid ACC offset index length: expected {expected_index_len}, got {}",
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

/// Returns how many consecutive transaction-free blocks one write-back carries.
///
/// `0` and `1` both mean "persist every block on its own", which is the
/// historical behaviour and the default. A value of two or more coalesces runs
/// of that length; anything above what the persistence layer accepts is clamped
/// by [`clamp_empty_batch_size`] before it reaches a write-back.
fn import_empty_batch_size_from_env() -> usize {
    std::env::var("NEO_IMPORT_EMPTY_BATCH_SIZE")
        .ok()
        .and_then(|raw| raw.trim().parse::<usize>().ok())
        .unwrap_or(0)
}

fn import_flush_interval_from_env() -> u64 {
    std::env::var("NEO_IMPORT_FLUSH_INTERVAL")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(10_000)
}

/// Returns whether a one-time full-range compaction must run before importing.
///
/// Resuming an import on a database written by a build that suppressed
/// background compaction leaves a large level-0 backlog behind. Draining it
/// once up front is much cheaper than paying the read amplification on every
/// subsequent block, so it is available as an explicit opt-in maintenance step.
fn import_compact_on_start_from_env() -> bool {
    std::env::var("NEO_IMPORT_COMPACT_ON_START")
        .ok()
        .map(|raw| {
            let value = raw.trim();
            value.eq_ignore_ascii_case("1")
                || value.eq_ignore_ascii_case("true")
                || value.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

/// Returns whether persistence-stage profiling was requested for this import.
///
/// Stage timings are opt-in because every sampled stage reads two clocks around
/// work the pipeline would otherwise perform untouched. When enabled, the
/// import starts its own sample so the reported deltas describe exactly this
/// run instead of the process lifetime.
fn import_persistence_profile_from_env() -> bool {
    std::env::var("NEO_IMPORT_PERSISTENCE_PROFILE")
        .ok()
        .map(|raw| {
            let value = raw.trim();
            value.eq_ignore_ascii_case("1")
                || value.eq_ignore_ascii_case("true")
                || value.eq_ignore_ascii_case("yes")
        })
        .unwrap_or(false)
}

/// The persisted current-block record: the height a restart would resume from
/// together with the block hash recorded for that height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct PersistedCheckpoint {
    height: u32,
    hash: UInt256,
}

fn read_persisted_checkpoint(cache: &StoreCache) -> PersistedCheckpoint {
    let ledger = LedgerContract::new();
    PersistedCheckpoint {
        height: ledger.current_index(cache).unwrap_or(0),
        hash: ledger.current_hash(cache).unwrap_or_default(),
    }
}

/// Reads the checkpoint from the current in-process store snapshot.
fn read_local_checkpoint(system: &Arc<NeoSystem>) -> PersistedCheckpoint {
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
fn verify_persisted_checkpoint(
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
fn verify_logs_watermark(
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

fn flush_and_verify_checkpoint(
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

/// One decoded archive record that the chain has not accepted yet.
struct DecodedBlock {
    meta: BlockMeta,
    block: Block,
}

/// What one successful persistence unit accepted.
struct UnitOutcome {
    accepted: u64,
    height: u32,
    hash: UInt256,
}

/// Brackets one chain commit with an ApplicationLogs persistence unit.
///
/// Records the committed callbacks produce for the blocks of this unit are
/// staged instead of written, so a chain commit that fails can discard them: the
/// logs database then never claims a block the chain store refused. A unit is
/// opened for every block (and every coalesced run) and closed before the next
/// decode begins.
///
/// Closing a unit only *promotes* its records. Inside an import session the
/// commit happens at the next checkpoint, which is what turns one logs write per
/// block into one logs write per checkpoint interval.
struct LogsUnit {
    logs: Option<Arc<ApplicationLogsService>>,
}

impl LogsUnit {
    fn begin(system: &Arc<NeoSystem>) -> Result<Self> {
        let logs = system
            .get_service::<ApplicationLogsService>()
            .ok()
            .flatten();
        if let Some(logs) = logs.as_ref() {
            match logs.begin_persistence_unit() {
                PersistenceUnitOutcome::Started => {}
                other => bail!("application logs persistence unit did not start: {other:?}"),
            }
        }
        Ok(Self { logs })
    }

    /// Promotes this unit's records to the session's accepted set.
    ///
    /// Nothing is committed here: the checkpoint decides when the accumulated
    /// records become durable, and the session's byte cap can force an earlier
    /// commit when the pending set would otherwise grow without bound.
    fn finish(self) -> Result<()> {
        let Some(logs) = self.logs else {
            return Ok(());
        };
        match logs
            .finish_persistence_unit()
            .map_err(|err| anyhow::anyhow!(err))?
        {
            PersistenceUnitOutcome::Finished { .. } => Ok(()),
            other => bail!("application logs persistence unit did not finish: {other:?}"),
        }
    }

    fn abort(self) {
        if let Some(logs) = self.logs {
            logs.abort_persistence_unit();
        }
    }
}

/// Returns the accepted-record byte cap an import session uses.
///
/// `NEO_IMPORT_LOGS_BYTE_CAP` bounds how much application-log data one session
/// accumulates between two checkpoints. Zero commits after every finished unit,
/// which is the historical per-block write pattern; the default is
/// [`DEFAULT_ACCEPTED_BYTE_CAP`].
fn import_logs_byte_cap_from_env() -> u64 {
    std::env::var("NEO_IMPORT_LOGS_BYTE_CAP")
        .ok()
        .and_then(|raw| raw.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_ACCEPTED_BYTE_CAP)
}

/// Import-scoped ApplicationLogs session.
///
/// Opening the session switches the service into batching mode: a finished unit
/// promotes its records and only a checkpoint commits them. The session is also
/// what makes the logs watermark mean "durable up to this height", so the
/// checkpoint can check it against the chain.
struct LogsSession {
    logs: Option<Arc<ApplicationLogsService>>,
}

impl LogsSession {
    /// Opens the session, or does nothing when no logs service is registered.
    fn begin(system: &Arc<NeoSystem>, chain_height: u32, chain_hash: &UInt256) -> Result<Self> {
        let logs = system
            .get_service::<ApplicationLogsService>()
            .ok()
            .flatten();
        if let Some(logs) = logs.as_ref() {
            logs.set_accepted_byte_cap(import_logs_byte_cap_from_env());
            match logs.begin_import_session() {
                ImportSessionOutcome::Started => {}
                other => bail!("application logs import session did not start: {other:?}"),
            }
            // Known limitation: a prefix whose logs were cut short by a crash
            // between the chain flush and the logs flush is detected here but not
            // repaired. Full resume validation is future work, so the import
            // continues and the gap stays visible in the warning.
            if chain_height > 0 && logs.get_block_log(chain_hash).is_none() {
                warn!(
                    target: "neo",
                    chain_height,
                    chain_hash = %chain_hash,
                    "application logs hold no record for the local chain tip; the logs store trails the chain and resume validation is not implemented yet (continuing)"
                );
            }
        }
        Ok(Self { logs })
    }

    /// Commits whatever is still pending and leaves batching mode.
    ///
    /// Returns the session's logs telemetry so the import summary can report it.
    /// The counters stay readable after the session ends, which is why they are
    /// read here instead of being reset.
    fn end(self) -> Result<LogsDeltas> {
        let Some(logs) = self.logs else {
            return Ok(LogsDeltas::default());
        };
        match logs
            .end_import_session()
            .map_err(|err| anyhow::anyhow!(err))?
        {
            ImportSessionOutcome::Ended { .. } | ImportSessionOutcome::NotOpen => {}
            other => bail!("application logs import session did not end: {other:?}"),
        }
        let telemetry = logs.flush_telemetry();
        Ok(LogsDeltas {
            flush_count: telemetry.flushes,
            flushed_records: telemetry.records,
            flushed_bytes: telemetry.bytes,
            pending_records: telemetry.pending_records,
        })
    }
}

/// Rejects a block that does not name the accepted tip as its predecessor.
fn preflight_link(parent: Option<UInt256>, meta: &BlockMeta) -> Result<()> {
    if let Some(parent) = parent
        && meta.prev_hash != parent
    {
        bail!(
            "block {} does not extend the accepted tip: prev_hash {} does not match {}",
            meta.index,
            meta.prev_hash,
            parent
        );
    }
    Ok(())
}

/// Persists one decoded block through the single-block path inside a unit.
fn persist_single_block(
    system: &Arc<NeoSystem>,
    parent: Option<UInt256>,
    decoded: DecodedBlock,
) -> Result<UnitOutcome> {
    let meta = decoded.meta;
    preflight_link(parent, &meta)?;
    let unit = LogsUnit::begin(system)?;
    match system.persist_block_without_runtime_cache(decoded.block) {
        Ok(_) => {
            unit.finish()
                .with_context(|| format!("failed to record the logs of block {}", meta.index))?;
            Ok(UnitOutcome {
                accepted: 1,
                height: meta.index,
                hash: meta.hash,
            })
        }
        Err(err) => {
            // A failure after execution started poisons the system: it must not
            // be retried, and no serial fallback may run. The unit is aborted so
            // nothing is left claiming this block.
            let poisoned = system.is_persistence_poisoned();
            unit.abort();
            Err(anyhow::anyhow!(err)).with_context(|| {
                format!(
                    "failed to persist imported block {} (persistence poisoned: {poisoned})",
                    meta.index
                )
            })
        }
    }
}

/// Persists a decoded run of transaction-free blocks as one write-back.
///
/// The run is validated *before* the persistence layer starts speculative
/// execution, so a run that is not a contiguous, predecessor-linked chain of
/// transaction-free blocks is rejected without poisoning the system.
fn persist_empty_run(
    system: &Arc<NeoSystem>,
    parent: Option<UInt256>,
    run: Vec<DecodedBlock>,
    trigger: DrainTrigger,
) -> Result<UnitOutcome> {
    let metas = run.iter().map(|decoded| decoded.meta).collect::<Vec<_>>();
    let last = *metas
        .last()
        .expect("a drained run always contains at least one block");
    validate_empty_run(parent, &metas)
        .map_err(|err| anyhow::anyhow!(err))
        .with_context(|| {
            format!(
                "empty-block run ending at {} rejected before persistence ({})",
                last.index,
                trigger.as_str()
            )
        })?;
    let accepted = u64::try_from(metas.len()).unwrap_or(u64::MAX);
    let blocks = run.into_iter().map(|decoded| decoded.block).collect();

    let unit = LogsUnit::begin(system)?;
    match system.persist_consecutive_empty_blocks_without_runtime_cache(blocks) {
        Ok(_) => {
            unit.finish().with_context(|| {
                format!(
                    "failed to record the logs of the {accepted} empty blocks ending at {}",
                    last.index
                )
            })?;
            Ok(UnitOutcome {
                accepted,
                height: last.index,
                hash: last.hash,
            })
        }
        Err(err) => {
            let poisoned = system.is_persistence_poisoned();
            unit.abort();
            Err(anyhow::anyhow!(err)).with_context(|| {
                format!(
                    "failed to persist {accepted} empty blocks ending at {} (persistence poisoned: {poisoned})",
                    last.index
                )
            })
        }
    }
}

/// Mutable state of one `.acc` replay.
///
/// The three frontiers are explicit:
///
/// - **decoded**: `current_height + pending.len()`, the height of the newest
///   record read ahead of the chain,
/// - **accepted**: `current_height`, advanced only when a persistence unit
///   returns successfully,
/// - **verified-durable**: `last_verified_checkpoint`, advanced only by a checked
///   checkpoint flush.
///
/// `imported` therefore counts accepted blocks, never decoded ones, so a failed
/// run cannot report a height it did not commit.
struct ImportRun<'a> {
    system: &'a Arc<NeoSystem>,
    storage_path: Option<&'a str>,
    started_at: Instant,
    policy: ImportDrainPolicy,
    stop_height: Option<u32>,
    current_height: u32,
    imported: u64,
    skipped: u64,
    counters: ImportCounters,
    last_accepted_hash: UInt256,
    chain_parent: Option<UInt256>,
    last_verified_checkpoint: Option<PersistedCheckpoint>,
    pending: Vec<DecodedBlock>,
    empty_block_batches: u64,
    last_drain: Option<DrainTrigger>,
    progress_window_started_at: Instant,
    progress_window_height: u32,
    progress_window_counters: ImportCounters,
    progress_emissions: u64,
}

impl<'a> ImportRun<'a> {
    fn new(
        system: &'a Arc<NeoSystem>,
        storage_path: Option<&'a str>,
        started_at: Instant,
        policy: ImportDrainPolicy,
        stop_height: Option<u32>,
    ) -> Self {
        // The parent of the first decoded block is the tip this process starts
        // from. It is only unknown for a store that has no current block, in
        // which case the header validator stays responsible for that block
        // exactly as it is on the single-block path.
        let accepted_hash = read_local_checkpoint(system).hash;
        let chain_parent = (accepted_hash != UInt256::default()).then_some(accepted_hash);
        let current_height = system.current_block_index();
        Self {
            system,
            storage_path,
            started_at,
            policy,
            stop_height,
            current_height,
            imported: 0,
            skipped: 0,
            counters: ImportCounters::default(),
            last_accepted_hash: accepted_hash,
            chain_parent,
            last_verified_checkpoint: None,
            pending: Vec::new(),
            empty_block_batches: 0,
            last_drain: None,
            progress_window_started_at: started_at,
            progress_window_height: current_height,
            progress_window_counters: ImportCounters::default(),
            progress_emissions: 0,
        }
    }

    /// Height of the newest decoded record, including pending read-ahead.
    fn decoded_height(&self) -> u32 {
        let pending = u32::try_from(self.pending.len()).unwrap_or(u32::MAX);
        self.current_height.saturating_add(pending)
    }

    /// Height the next decoded record must carry.
    fn next_decoded_height(&self) -> Result<u32> {
        let pending = u32::try_from(self.pending.len()).unwrap_or(u32::MAX);
        self.current_height
            .checked_add(pending)
            .and_then(|height| height.checked_add(1))
            .ok_or_else(|| {
                anyhow::anyhow!("import height overflow: the archive exceeds u32::MAX blocks")
            })
    }

    fn last_pending_height(&self) -> u32 {
        self.pending
            .last()
            .map_or(self.current_height, |decoded| decoded.meta.index)
    }

    fn heartbeat_due(&self) -> bool {
        self.progress_window_started_at.elapsed().as_secs() >= self.policy.progress_heartbeat_secs
    }

    /// Persists the decoded run. Returns whether the import must stop.
    fn drain(&mut self, trigger: DrainTrigger) -> Result<bool> {
        if self.pending.is_empty() {
            return Ok(false);
        }
        let run = std::mem::take(&mut self.pending);
        let len = run.len();
        let outcome = if len == 1 {
            let decoded = run
                .into_iter()
                .next()
                .expect("a run of one block holds one block");
            persist_single_block(self.system, self.chain_parent, decoded).with_context(|| {
                format!(
                    "failed to persist the decoded empty block ({})",
                    trigger.as_str()
                )
            })?
        } else {
            persist_empty_run(self.system, self.chain_parent, run, trigger).with_context(|| {
                format!(
                    "failed to persist a decoded run of {len} empty blocks ({})",
                    trigger.as_str()
                )
            })?
        };
        self.accept(outcome);
        if len >= 2 {
            self.counters.record_empty_batch(len);
            self.empty_block_batches = self.empty_block_batches.saturating_add(1);
        } else {
            self.counters.record_block(0);
        }
        self.last_drain = Some(trigger);
        self.after_unit()
    }

    /// Persists one transaction-bearing block. Returns whether the import must stop.
    fn persist_one(&mut self, decoded: DecodedBlock) -> Result<bool> {
        let transaction_count = decoded.meta.transaction_count;
        let outcome = persist_single_block(self.system, self.chain_parent, decoded)?;
        self.accept(outcome);
        self.counters.record_block(transaction_count);
        self.last_drain = None;
        self.after_unit()
    }

    /// Advances the accepted frontier after a successful persistence unit.
    fn accept(&mut self, outcome: UnitOutcome) {
        self.current_height = outcome.height;
        self.last_accepted_hash = outcome.hash;
        self.chain_parent = Some(outcome.hash);
        self.imported = self.imported.saturating_add(outcome.accepted);
    }

    /// Runs the checkpoint, stop-height and progress steps of an accepted unit.
    fn after_unit(&mut self) -> Result<bool> {
        let periodic_flush_due = self.imported.is_multiple_of(self.policy.flush_interval);
        if periodic_flush_due {
            self.checkpoint("periodic")?;
        }

        if self
            .stop_height
            .is_some_and(|limit| self.current_height >= limit)
        {
            if !periodic_flush_due {
                self.checkpoint("stop-height")?;
            }
            info!(
                target: "neo",
                imported = self.imported,
                current_height = self.current_height,
                stop_height = ?self.stop_height,
                "reached NEO_IMPORT_STOP_HEIGHT; ending import early"
            );
            return Ok(true);
        }

        if self.imported.is_multiple_of(self.policy.progress_interval) || self.heartbeat_due() {
            self.emit_progress();
        }
        Ok(false)
    }

    fn checkpoint(&mut self, reason: &str) -> Result<()> {
        self.last_verified_checkpoint = Some(flush_and_verify_checkpoint(
            self.system,
            self.storage_path,
            self.current_height,
            &self.last_accepted_hash,
            self.imported,
            self.skipped,
            reason,
        )?);
        Ok(())
    }

    fn emit_progress(&mut self) {
        let window_elapsed = self.progress_window_started_at.elapsed();
        let run_elapsed = self.started_at.elapsed();
        let window = self.counters.delta_since(self.progress_window_counters);
        let window_height_delta =
            u64::from(self.current_height).saturating_sub(u64::from(self.progress_window_height));
        let local_view_height = read_local_checkpoint(self.system).height;
        let current_hash_written = LedgerContract::new()
            .get_block_hash_by_index(&self.system.context().store_cache(), self.current_height)
            .ok()
            .flatten()
            .is_some();
        // Level-0 file count is the cheapest signal that background compaction
        // is keeping up with the import write rate.
        let level_files = self.system.store().level_file_counts();
        let l0_files = level_files
            .iter()
            .find_map(|(level, count)| (*level == 0).then_some(*count))
            .unwrap_or(0);
        self.progress_emissions += 1;
        let decoded_height = self.decoded_height();
        // `rate_blocks_per_sec` keeps its historical meaning (lifetime average
        // for this run); `window_rate_blocks_per_sec` is the rate since the
        // previous emission only, so a stalled or speeding import is visible
        // instead of being averaged away.
        info!(
            target: "neo",
            imported = self.imported,
            skipped = self.skipped,
            current_height = self.current_height,
            decoded_height,
            local_view_height,
            current_hash_written,
            rate_blocks_per_sec = self.counters.blocks_per_sec(run_elapsed),
            window_rate_blocks_per_sec = window.blocks_per_sec(window_elapsed),
            window_secs = window_elapsed.as_secs_f64(),
            window_height_delta,
            window_imported_blocks = window.imported_blocks,
            window_empty_blocks = window.empty_blocks,
            window_non_empty_blocks = window.non_empty_blocks,
            window_transactions = window.transactions,
            window_empty_block_batches = window.empty_block_batches,
            window_batched_empty_blocks = window.batched_empty_blocks,
            imported_blocks = self.counters.imported_blocks,
            empty_blocks = self.counters.empty_blocks,
            non_empty_blocks = self.counters.non_empty_blocks,
            transactions = self.counters.transactions,
            empty_block_batches = self.counters.empty_block_batches,
            batched_empty_blocks = self.counters.batched_empty_blocks,
            batching_enabled = self.policy.batching_enabled(),
            empty_batch_cap = self.policy.empty_batch_cap,
            drain_trigger = self.last_drain.map(DrainTrigger::as_str),
            progress_emissions = self.progress_emissions,
            l0_files,
            level_files = ?level_files,
            "acc import progress"
        );
        self.progress_window_started_at = Instant::now();
        self.progress_window_height = self.current_height;
        self.progress_window_counters = self.counters;
    }
}

/// Reads and decodes one archive record.
///
/// Returns `None` for a record the local chain already holds, which is how an
/// indexed resume skips persisted heights without decoding them.
fn read_decoded_block(
    reader: &mut BufReader<File>,
    payload: &mut Vec<u8>,
    size_buf: &mut [u8; 4],
    declared_start: u32,
    item: u32,
    run: &mut ImportRun<'_>,
) -> Result<Option<DecodedBlock>> {
    reader
        .read_exact(&mut size_buf[..])
        .with_context(|| format!("failed to read block size at item {item}"))?;
    let block_size = u32::from_le_bytes(*size_buf) as usize;
    if block_size == 0 {
        bail!("encountered zero-sized block payload at item {item}");
    }

    let Some(declared_index) = declared_start.checked_add(item) else {
        bail!("declared block index overflow at item {item}");
    };

    if declared_index <= run.current_height {
        let skip_len = i64::try_from(block_size)
            .with_context(|| format!("block size too large to skip at item {item}"))?;
        reader
            .seek(SeekFrom::Current(skip_len))
            .with_context(|| format!("failed to seek past block payload at item {item}"))?;
        run.skipped += 1;
        if run.skipped.is_multiple_of(100_000) {
            info!(
                target: "neo",
                skipped = run.skipped,
                current_height = run.current_height,
                "acc import scan progress"
            );
        }
        return Ok(None);
    }

    payload.resize(block_size, 0u8);
    reader
        .read_exact(&mut payload[..block_size])
        .with_context(|| format!("failed to read block payload at item {item}"))?;

    let mut block_reader = MemoryReader::new(&payload[..block_size]);
    let mut block = <Block as Serializable>::deserialize(&mut block_reader)
        .with_context(|| format!("failed to deserialize block payload at item {item}"))?;
    let block_index = block.index();

    if block_index != declared_index {
        bail!(
            "import block index mismatch at item {item}: declared {declared_index}, decoded {block_index}"
        );
    }

    // The sequence check runs against the *decoded* frontier, because blocks that
    // are still pending are read ahead of the chain tip but already consumed
    // their archive slot.
    let expected = run.next_decoded_height()?;
    if block_index != expected {
        bail!("import sequence gap at item {item}: expected block {expected}, got {block_index}");
    }

    // The hash is computed from the decoded header before the block is moved into
    // persistence, so the checkpoint verified later must reproduce exactly this
    // value.
    let transaction_count = block.transactions.len();
    let hash = block
        .try_hash()
        .with_context(|| format!("failed to hash imported block {block_index}"))?;
    let prev_hash = *block.prev_hash();
    Ok(Some(DecodedBlock {
        meta: BlockMeta {
            index: block_index,
            hash,
            prev_hash,
            transaction_count,
        },
        block,
    }))
}

pub fn import_acc_file(
    system: &Arc<NeoSystem>,
    path: &Path,
    storage_path: Option<&str>,
) -> Result<ImportSummary> {
    import_acc_file_until(system, path, storage_path, import_stop_height_from_env())
}

/// Imports `.acc` records until `stop_height` (inclusive) is reached.
///
/// [`import_acc_file`] takes the limit from `NEO_IMPORT_STOP_HEIGHT`; the
/// explicit parameter keeps the boundary testable without mutating process-wide
/// environment variables.
fn import_acc_file_until(
    system: &Arc<NeoSystem>,
    path: &Path,
    storage_path: Option<&str>,
    stop_height: Option<u32>,
) -> Result<ImportSummary> {
    import_acc_file_with_options(system, path, storage_path, stop_height, None)
}

/// Imports `.acc` records with an explicit drain policy.
///
/// [`import_acc_file_until`] derives the policy from the process environment;
/// the explicit parameter keeps the batching boundaries testable without
/// mutating process-wide variables.
fn import_acc_file_with_options(
    system: &Arc<NeoSystem>,
    path: &Path,
    storage_path: Option<&str>,
    stop_height: Option<u32>,
    policy_override: Option<ImportDrainPolicy>,
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

    let current_height = system.current_block_index();
    let started_at = Instant::now();

    // Stop-height boundary. A resumed import whose local height already meets
    // the limit must not read or import another archive record, so this returns
    // before the fast-sync switch, before profiling starts and before any resume
    // seek into the payload area.
    if let Some(limit) = stop_height
        && current_height >= limit
    {
        info!(
            target: "neo",
            file = %path.display(),
            declared_start,
            declared_count,
            local_height = current_height,
            stop_height = ?stop_height,
            imported = 0u64,
            skipped = 0u64,
            // Nothing was persisted by this invocation, so no checkpoint claim
            // is made here and `elapsed_secs` reports zero import work.
            checkpoint_verified = false,
            "acc import stop height already reached; no archive record was read"
        );
        return Ok(ImportSummary {
            declared_start,
            declared_count,
            imported: 0,
            skipped: 0,
            final_height: current_height,
            elapsed_secs: 0.0,
            counters: ImportCounters::default(),
            fast_path: FastPathDeltas::default(),
            parallel: ParallelDeltas::default(),
            profile: PersistenceProfileDeltas::default(),
            logs: LogsDeltas::default(),
        });
    }

    let mut size_buf = [0u8; 4];
    let mut payload = Vec::<u8>::new();
    let progress_interval = import_progress_interval_from_env();
    let progress_heartbeat_secs = import_progress_heartbeat_secs_from_env();
    let flush_interval = import_flush_interval_from_env();

    // Empty-block batching is opt-in: `NEO_IMPORT_EMPTY_BATCH_SIZE` defaults to
    // zero, which keeps the historical one-block-per-call path. A configured
    // size above what the persistence layer accepts is clamped rather than
    // ignored, so the effective value is always reported.
    let (configured_empty_batch_size, empty_batch_cap_clamped) =
        clamp_empty_batch_size(import_empty_batch_size_from_env());
    if empty_batch_cap_clamped {
        warn!(
            target: "neo",
            configured_empty_batch_size,
            max_empty_batch_size = MAX_EMPTY_BATCH_SIZE,
            "NEO_IMPORT_EMPTY_BATCH_SIZE exceeds the persistence write-back limit; using the maximum"
        );
    }
    let policy = policy_override.unwrap_or_else(|| {
        ImportDrainPolicy::new(
            configured_empty_batch_size,
            flush_interval,
            progress_interval,
            progress_heartbeat_secs,
        )
    });
    let empty_batch_cap = policy.empty_batch_cap;

    // Run state. `imported` counts accepted blocks only, and `run` owns every
    // frontier so a failed unit can never advance the imported count.
    let mut run = ImportRun::new(system, storage_path, started_at, policy, stop_height);

    // ApplicationLogs batching session. Opening it switches the service from one
    // commit per block to one commit per checkpoint: every finished unit only
    // promotes its records, and the checkpoints below decide when they become
    // durable. The session opens after the stop-height return so an invocation
    // that imports nothing never leaves one behind.
    let mut logs_session = Some(LogsSession::begin(
        system,
        run.current_height,
        &run.last_accepted_hash,
    )?);

    // Import is an offline bootstrap path: keep fast-sync mode enabled to skip
    // expensive event fan-out while preserving full block execution/validation.
    let was_fast_sync = system.context().is_fast_sync_mode();
    if !was_fast_sync {
        system.context().enable_fast_sync_mode();
    }

    // Optional one-time maintenance: drain a level-0 backlog inherited from a
    // previous run before switching the store to WAL-less writes.
    if import_compact_on_start_from_env() {
        let level_files_before = system.store().level_file_counts();
        let compact_started_at = Instant::now();
        info!(
            target: "neo",
            level_files = ?level_files_before,
            "starting pre-import full-range compaction"
        );
        system.store().compact_all();
        info!(
            target: "neo",
            level_files_before = ?level_files_before,
            level_files_after = ?system.store().level_file_counts(),
            elapsed_ms = compact_started_at.elapsed().as_millis(),
            "pre-import full-range compaction completed"
        );
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
        empty_batch_cap,
        batching_enabled = policy.batching_enabled(),
        "starting .acc import"
    );

    // Persistence-stage timings are opt-in: every sampled stage reads two clocks
    // around work the pipeline performs anyway, so the import only starts a
    // sample when asked for one. Enabling here (instead of relying on the usual
    // metrics pump, which is never started for `--import-only`) is what makes the
    // summary deltas meaningful for an offline import.
    let stats = system.execution_stats();
    let profiling_started_by_import =
        import_persistence_profile_from_env() && !stats.profile_snapshot().enabled;
    if profiling_started_by_import {
        system.set_execution_profiling(true);
    }
    let fast_path_start = stats.fast_path_snapshot();
    let parallel_start = stats.parallel_snapshot();
    let profile_start = stats.profile_snapshot();

    let first_item = if let Some(resume) =
        indexed_resume_position(path, declared_start, declared_count, current_height)?
    {
        reader
            .seek(SeekFrom::Start(resume.offset))
            .context("failed to seek ACC import to the persisted height")?;
        run.skipped = resume.skipped;
        info!(
            target: "neo",
            resume_height = current_height.saturating_add(1),
            first_item = resume.first_item,
            skipped = run.skipped,
            offset = resume.offset,
            seek_elapsed_ms = started_at.elapsed().as_millis(),
            "acc import offset seek completed"
        );
        resume.first_item
    } else {
        0
    };

    // Replays the archive. `skipped` counts archive records that were already
    // persisted before this run; those never reach `counters`, so a resumed
    // import never reports pre-existing blocks as imported, empty, non-empty or
    // transaction-bearing.
    //
    // Transaction-free blocks are decoded ahead of the chain and persisted as one
    // write-back per decoded run. Every run ends on a boundary the single-block
    // path would have hit on its own: a checkpoint multiple, the stop height, a
    // progress window, a transaction-bearing successor, the end of the archive or
    // a record that cannot be read.
    let loop_result: Result<()> = (|| {
        for i in first_item..declared_count {
            let decoded = match read_decoded_block(
                &mut reader,
                &mut payload,
                &mut size_buf,
                declared_start,
                i,
                &mut run,
            ) {
                Ok(Some(decoded)) => decoded,
                Ok(None) => continue,
                Err(err) => {
                    // A record that cannot be read or decoded ends the decoded
                    // run. The blocks read before it are still persisted, so a
                    // failed import leaves exactly the heights the single-block
                    // path would have left, then reports the failure. A drain
                    // failure takes precedence: it means the run itself could not
                    // be persisted.
                    run.drain(DrainTrigger::ReadFailure)?;
                    return Err(err);
                }
            };

            let stop = if decoded.meta.transaction_count == 0 {
                run.pending.push(decoded);
                match empty_run_drain_trigger(
                    run.pending.len(),
                    run.imported,
                    run.last_pending_height(),
                    run.stop_height,
                    run.heartbeat_due(),
                    run.policy,
                ) {
                    Some(trigger) => run.drain(trigger)?,
                    None => false,
                }
            } else if run.drain(DrainTrigger::NonEmptySuccessor)? {
                // The stop height was reached while draining, so this
                // transaction-bearing block must not be persisted.
                true
            } else {
                run.persist_one(decoded)?
            };

            if stop {
                break;
            }
        }

        // The archive ended (EOF or the declared count), so whatever is still
        // decoded is persisted instead of being carried into the completion
        // path.
        run.drain(DrainTrigger::EndOfArchive)?;
        Ok(())
    })();

    // Restore write mode before the checkpoint work, so the completion path
    // never leaves the store configured for offline-only writes.
    system.store().disable_fast_sync_mode();
    if !was_fast_sync {
        system.context().disable_fast_sync_mode();
    }

    let outcome: Result<ImportSummary> = (|| {
        loop_result?;

        // The observation windows close as soon as the replay stops, so the
        // reported deltas describe the blocks this run persisted and nothing
        // that happened before it.
        let fast_path_end = stats.fast_path_snapshot();
        let parallel_end = stats.parallel_snapshot();
        let profile_end = stats.profile_snapshot();

        // Nothing is reported complete until the final checkpoint is durable
        // *and* its persisted hash is the exact hash of the last accepted
        // block. A checkpoint already verified at this exact height and hash
        // needs no second flush.
        let expected_checkpoint = PersistedCheckpoint {
            height: run.current_height,
            hash: run.last_accepted_hash,
        };
        let final_checkpoint = if run.last_verified_checkpoint == Some(expected_checkpoint) {
            expected_checkpoint
        } else {
            flush_and_verify_checkpoint(
                system,
                storage_path,
                run.current_height,
                &run.last_accepted_hash,
                run.imported,
                run.skipped,
                "final",
            )?
        };

        // Close the session. Everything the final checkpoint left pending is
        // committed here, so the summary reports a settled logs store instead of
        // records that would only ever exist in memory.
        let logs_deltas = match logs_session.take() {
            Some(session) => session
                .end()
                .context("failed to close the application logs import session")?,
            None => LogsDeltas::default(),
        };

        // Timing is taken after the final verified checkpoint, so the reported
        // duration covers the durable end state rather than an unflushed tip.
        let elapsed = started_at.elapsed();
        let elapsed_secs = elapsed.as_secs_f64();
        let fast_path = FastPathDeltas::between(fast_path_start, fast_path_end);
        let parallel = ParallelDeltas::between(parallel_start, parallel_end);
        let profile = PersistenceProfileDeltas::between(&profile_start, &profile_end);

        info!(
            target: "neo",
            file = %path.display(),
            declared_start,
            declared_count,
            imported = run.imported,
            skipped = run.skipped,
            final_height = run.current_height,
            elapsed_secs,
            rate_blocks_per_sec = run.counters.blocks_per_sec(elapsed),
            final_height_hash = %final_checkpoint.hash,
            final_checkpoint_verified = true,
            imported_blocks = run.counters.imported_blocks,
            empty_blocks = run.counters.empty_blocks,
            non_empty_blocks = run.counters.non_empty_blocks,
            transactions = run.counters.transactions,
            empty_block_batches = run.counters.empty_block_batches,
            batched_empty_blocks = run.counters.batched_empty_blocks,
            batching_enabled = policy.batching_enabled(),
            empty_batch_cap = policy.empty_batch_cap,
            progress_emissions = run.progress_emissions,
            logs_flush_count = logs_deltas.flush_count,
            logs_flushed_records = logs_deltas.flushed_records,
            logs_flushed_bytes = logs_deltas.flushed_bytes,
            logs_pending_records = logs_deltas.pending_records,
            fast_path_direct_attempts = fast_path.direct_attempts,
            fast_path_direct_halts = fast_path.direct_halts,
            fast_path_direct_faults = fast_path.direct_faults,
            fast_path_eligibility_declines = fast_path.eligibility_declines,
            fast_path_canonical_fallbacks = fast_path.canonical_fallbacks,
            fast_path_verification_mismatches = fast_path.verification_mismatches,
            parallel_speculative_blocks = parallel.speculative_blocks,
            parallel_speculative_transactions = parallel.speculative_transactions,
            parallel_conflicts = parallel.conflicts,
            parallel_reexecutions = parallel.reexecutions,
            parallel_serial_fallbacks = parallel.serial_fallbacks,
            parallel_elapsed_ms = nanos_to_millis(parallel.parallel_nanos),
            serial_elapsed_ms = nanos_to_millis(parallel.serial_nanos),
            profile_state = profile.state_label(),
            profile_enabled_at_end = profile.end_enabled,
            profile_blocks = profile.blocks,
            profile_transaction_executions = profile.transaction_executions,
            profile_external_instructions = profile.external_instructions,
            profile_snapshot_open_ms = nanos_to_millis(profile.snapshot_open_nanos),
            profile_on_persist_ms = nanos_to_millis(profile.on_persist_nanos),
            profile_transaction_loop_ms = nanos_to_millis(profile.transaction_loop_nanos),
            profile_post_persist_ms = nanos_to_millis(profile.post_persist_nanos),
            profile_state_root_stage_ms = nanos_to_millis(profile.participant_precommit_nanos),
            profile_chain_commit_ms = nanos_to_millis(profile.store_commit_nanos),
            profile_transaction_hash_ms = nanos_to_millis(profile.transaction_hash_nanos),
            profile_transaction_vm_ms = nanos_to_millis(profile.transaction_vm_nanos),
            profile_state_change_extraction_ms =
                nanos_to_millis(profile.state_change_extraction_nanos),
            profile_mpt_ms = nanos_to_millis(profile.mpt_nanos),
            profile_store_writer_snapshot_open_ms =
                nanos_to_millis(profile.store_writer_snapshot_open_nanos),
            profile_store_change_application_ms =
                nanos_to_millis(profile.store_change_application_nanos),
            profile_store_write_batch_build_ms =
                nanos_to_millis(profile.store_write_batch_build_nanos),
            profile_store_cache_invalidation_ms =
                nanos_to_millis(profile.store_cache_invalidation_nanos),
            profile_store_database_write_ms =
                nanos_to_millis(profile.store_database_write_nanos),
            profile_runtime_update_ms = nanos_to_millis(profile.runtime_update_nanos),
            profile_post_commit_ms = nanos_to_millis(profile.post_commit_nanos),
            profile_mpt_update_ms = nanos_to_millis(profile.mpt_update_nanos),
            profile_mpt_commit_ms = nanos_to_millis(profile.mpt_commit_nanos),
            profile_mpt_root_hash_ms = nanos_to_millis(profile.mpt_root_hash_nanos),
            profile_mpt_hash_ms = nanos_to_millis(profile.mpt_hash_nanos),
            profile_mpt_serialization_ms = nanos_to_millis(profile.mpt_serialization_nanos),
            profile_mpt_hash256_ms = nanos_to_millis(profile.mpt_hash256_nanos),
            profile_mpt_node_cache_ms = nanos_to_millis(profile.mpt_node_cache_nanos),
            profile_mpt_node_read_ms = nanos_to_millis(profile.mpt_node_read_nanos),
            profile_mpt_node_deserialize_ms = nanos_to_millis(profile.mpt_node_deserialize_nanos),
            profile_mpt_node_encode_ms = nanos_to_millis(profile.mpt_node_encode_nanos),
            profile_mpt_node_write_ms = nanos_to_millis(profile.mpt_node_write_nanos),
            profile_empty_block_blocks = profile.empty_blocks.blocks,
            profile_empty_block_snapshot_open_ms =
                nanos_to_millis(profile.empty_blocks.snapshot_open_nanos),
            profile_empty_block_on_persist_ms =
                nanos_to_millis(profile.empty_blocks.on_persist_nanos),
            profile_empty_block_transaction_loop_ms =
                nanos_to_millis(profile.empty_blocks.transaction_loop_nanos),
            profile_empty_block_post_persist_ms =
                nanos_to_millis(profile.empty_blocks.post_persist_nanos),
            profile_empty_block_state_root_stage_ms =
                nanos_to_millis(profile.empty_blocks.participant_precommit_nanos),
            profile_empty_block_state_change_extraction_ms =
                nanos_to_millis(profile.empty_blocks.state_change_extraction_nanos),
            profile_empty_block_mpt_ms = nanos_to_millis(profile.empty_blocks.mpt_nanos),
            profile_empty_block_mpt_update_ms =
                nanos_to_millis(profile.empty_blocks.mpt_update_nanos),
            profile_empty_block_mpt_commit_ms =
                nanos_to_millis(profile.empty_blocks.mpt_commit_nanos),
            profile_empty_block_mpt_hash_ms =
                nanos_to_millis(profile.empty_blocks.mpt_hash_nanos),
            profile_empty_block_mpt_serialization_ms =
                nanos_to_millis(profile.empty_blocks.mpt_serialization_nanos),
            profile_empty_block_mpt_hash256_ms =
                nanos_to_millis(profile.empty_blocks.mpt_hash256_nanos),
            profile_empty_block_mpt_node_cache_ms =
                nanos_to_millis(profile.empty_blocks.mpt_node_cache_nanos),
            profile_empty_block_mpt_node_read_ms =
                nanos_to_millis(profile.empty_blocks.mpt_node_read_nanos),
            profile_empty_block_mpt_node_deserialize_ms =
                nanos_to_millis(profile.empty_blocks.mpt_node_deserialize_nanos),
            profile_empty_block_mpt_node_encode_ms =
                nanos_to_millis(profile.empty_blocks.mpt_node_encode_nanos),
            profile_empty_block_mpt_node_write_ms =
                nanos_to_millis(profile.empty_blocks.mpt_node_write_nanos),
            profile_empty_block_chain_commit_ms =
                nanos_to_millis(profile.empty_blocks.store_commit_nanos),
            profile_empty_block_store_database_write_ms =
                nanos_to_millis(profile.empty_blocks.store_database_write_nanos),
            profile_empty_block_runtime_update_ms =
                nanos_to_millis(profile.empty_blocks.runtime_update_nanos),
            profile_empty_block_post_commit_ms =
                nanos_to_millis(profile.empty_blocks.post_commit_nanos),
            profile_stage_nesting = PERSISTENCE_STAGE_NESTING,
            "completed .acc import"
        );

        Ok(ImportSummary {
            declared_start,
            declared_count,
            imported: run.imported,
            skipped: run.skipped,
            final_height: run.current_height,
            elapsed_secs,
            counters: run.counters,
            fast_path,
            parallel,
            profile,
            logs: logs_deltas,
        })
    })();

    if outcome.is_err() {
        // Best effort only: this path never reports the import as complete, so
        // the fail-closed durable flush above remains the only success gate.
        system.store().flush();
        // Preserve whatever state roots were computed before the failure so a
        // restart can still serve them (the state store is a separate store
        // and is not covered by `system.store().flush()`).
        if let Ok(Some(state_store)) = system.state_store() {
            state_store.flush();
        }
        // Closing the session writes whatever the finished units promoted. Those
        // records belong to a chain state that was already committed, so they are
        // kept even though the import itself failed; the unit of the failing block
        // was aborted and contributes nothing.
        if let Some(session) = logs_session.take()
            && let Err(err) = session.end()
        {
            warn!(
                target: "neo",
                error = %err,
                "failed to write accepted application logs after an import error"
            );
        }
        if let Ok(Some(logs)) = system.get_service::<ApplicationLogsService>() {
            logs.flush();
        }
        if let Ok(Some(tracker)) =
            system.get_service::<neo_rpc::tokens_tracker::TokensTrackerService>()
        {
            tracker.flush();
        }
        info!(
            target: "neo",
            "storage flush completed after .acc import error"
        );
    }

    // Profiling is switched on by this import only when it asked for it, so an
    // opt-in observation run never leaves the runtime instrumented.
    if profiling_started_by_import {
        system.set_execution_profiling(false);
    }

    outcome
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_primitives::UInt160;
    use neo_protocol::payloads::Header;
    use neo_protocol::witness::Witness;
    use neo_storage::persistence::read_only_store::{ReadOnlyStore, ReadOnlyStoreGeneric};
    use neo_storage::persistence::store::OnNewSnapshotDelegate;
    use neo_storage::persistence::write_store::WriteStore;
    use neo_storage::persistence::{SeekDirection, Store, StoreSnapshot};
    use neo_storage::{StorageError, StorageItem, StorageKey, StorageResult};
    use neo_system::i_event_handlers::{CommittedHandler, CommittingHandler};
    use std::io::Write;
    use std::sync::Mutex;
    use std::sync::atomic::{AtomicBool, Ordering};

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

    fn memory_system() -> Arc<NeoSystem> {
        NeoSystem::new(neo_protocol::ProtocolSettings::default(), None, None)
            .expect("memory-backed neo system")
    }

    fn hash_of(byte: u8) -> UInt256 {
        UInt256::from_bytes(&[byte; 32]).expect("32-byte hash")
    }

    #[tokio::test]
    async fn stop_height_already_reached_imports_zero_blocks() {
        let system = memory_system();
        let start_height = system.current_block_index();

        // The header declares three records but no payload bytes follow it, so
        // any attempt to read a record would fail. Returning a summary is
        // therefore proof that the importer stopped before touching the
        // archive.
        let temp = tempfile::tempdir().expect("temporary directory");
        let acc_path = temp.path().join("chain.0.acc");
        {
            let mut acc = File::create(&acc_path).expect("create ACC");
            acc.write_all(&start_height.to_le_bytes())
                .expect("ACC start");
            acc.write_all(&3u32.to_le_bytes()).expect("ACC count");
        }

        let summary = import_acc_file_until(&system, &acc_path, None, Some(start_height))
            .expect("a stop height that is already met must not fail");

        assert_eq!(summary.imported, 0);
        assert_eq!(summary.skipped, 0);
        assert_eq!(summary.declared_start, start_height);
        assert_eq!(summary.declared_count, 3);
        assert_eq!(summary.final_height, start_height);
        assert_eq!(summary.elapsed_secs, 0.0);
        assert_eq!(summary.counters, ImportCounters::default());
        assert_eq!(summary.fast_path, FastPathDeltas::default());
        assert_eq!(summary.parallel, ParallelDeltas::default());
        assert_eq!(summary.profile.state_label(), "disabled");
        assert_eq!(system.current_block_index(), start_height);
    }

    #[test]
    fn persisted_checkpoint_must_carry_the_exact_accepted_hash() {
        let accepted = hash_of(0x11);
        let error = verify_persisted_checkpoint(
            "unit-test",
            "disk",
            7,
            &accepted,
            PersistedCheckpoint {
                height: 7,
                hash: hash_of(0x22),
            },
        )
        .expect_err("a different hash at the right height must be rejected");

        let message = error.to_string();
        assert!(
            message.contains("block hash"),
            "unexpected error: {message}"
        );
        assert!(
            message.contains("does not match the last accepted block hash"),
            "unexpected error: {message}"
        );
    }

    #[test]
    fn persisted_checkpoint_must_carry_the_accepted_height() {
        let accepted = hash_of(0x11);
        let error = verify_persisted_checkpoint(
            "unit-test",
            "local",
            7,
            &accepted,
            PersistedCheckpoint {
                height: 6,
                hash: accepted,
            },
        )
        .expect_err("a stale height must be rejected");

        assert!(
            error
                .to_string()
                .contains("does not match the last accepted block height"),
            "unexpected error: {error}"
        );
    }

    #[test]
    fn persisted_checkpoint_matching_height_and_hash_is_accepted() {
        let accepted = hash_of(0x11);
        verify_persisted_checkpoint(
            "unit-test",
            "disk",
            7,
            &accepted,
            PersistedCheckpoint {
                height: 7,
                hash: accepted,
            },
        )
        .expect("the accepted block checkpoint must verify");
    }

    #[tokio::test]
    async fn checkpoint_success_requires_a_durable_flush() {
        let system = memory_system();
        let expected = read_local_checkpoint(&system);

        // A best-effort `flush()` would silently succeed here; only the
        // fallible durable flush rejects a backend without durable writes.
        let error = flush_and_verify_checkpoint(
            &system,
            None,
            expected.height,
            &expected.hash,
            0,
            0,
            "unit-test",
        )
        .expect_err("a store without durable flush support must not report a checkpoint");

        assert!(
            error
                .to_string()
                .contains("durable chain-store flush failed"),
            "unexpected error: {error}"
        );
    }

    #[tokio::test]
    async fn local_checkpoint_tracks_the_persisted_genesis_tip() {
        let system = memory_system();
        let checkpoint = read_local_checkpoint(&system);
        assert_eq!(checkpoint.height, system.current_block_index());

        let mut genesis = neo_ledger::create_genesis_block(system.settings());
        assert_eq!(checkpoint.hash, genesis.hash());
    }

    #[tokio::test]
    async fn persistence_profiling_is_disabled_until_explicitly_requested() {
        let system = memory_system();
        let stats = system.execution_stats();
        let start = stats.profile_snapshot();
        assert!(!start.enabled);

        // Every requested stage collapses to a labelled zero rather than an
        // implied "measured zero cost" when profiling is off.
        let deltas = PersistenceProfileDeltas::between(&start, &stats.profile_snapshot());
        assert_eq!(deltas.state_label(), "disabled");
        assert_eq!(deltas.blocks, 0);
        assert_eq!(deltas.transaction_executions, 0);
        assert_eq!(deltas.snapshot_open_nanos, 0);
        assert_eq!(deltas.on_persist_nanos, 0);
        assert_eq!(deltas.transaction_loop_nanos, 0);
        assert_eq!(deltas.post_persist_nanos, 0);
        assert_eq!(deltas.participant_precommit_nanos, 0);
        assert_eq!(deltas.store_commit_nanos, 0);
        assert!(!stats.profile_snapshot().enabled);
    }

    /// Application-logs store wrapper that observes a checkpoint's durability
    /// gate without changing what is stored.
    ///
    /// Reads, snapshots and the real durable flush are delegated to the wrapped
    /// store, so records land exactly where they do in production. Only the flush
    /// is intercepted: it records what the logs database already held and how far
    /// the durable watermark stood *before* the flush, which is what makes the
    /// checkpoint ordering observable. Arming it makes the next flush fail, which
    /// is how a test proves a checkpoint refuses to certify an undurable logs
    /// database.
    struct ObservedLogsStore {
        inner: Arc<dyn Store>,
        armed: AtomicBool,
        /// Whether the wrapped backend really implements an explicit durable
        /// flush, which decides what an unarmed flush reports.
        delegates_flush: bool,
        observations: Arc<Mutex<Vec<FlushObservation>>>,
        service: Mutex<Option<std::sync::Weak<ApplicationLogsService>>>,
    }

    /// What one durable flush saw before it ran.
    #[derive(Debug, Clone, Copy)]
    struct FlushObservation {
        /// Records already in the logs database when the flush started.
        stored_records: usize,
        /// Durable watermark when the flush started.
        watermark: Option<(u32, UInt256)>,
    }

    impl ObservedLogsStore {
        /// Wraps a backend that implements an explicit durable flush, so the flush
        /// this records is the one the backend actually runs.
        fn new(inner: Arc<dyn Store>, observations: Arc<Mutex<Vec<FlushObservation>>>) -> Self {
            Self {
                inner,
                armed: AtomicBool::new(false),
                delegates_flush: true,
                observations,
                service: Mutex::new(None),
            }
        }

        /// Wraps a backend that cannot fsync and stands in for one that can.
        ///
        /// A memory store has no durable-flush support to delegate to, so an
        /// unarmed flush reports success instead. That is what lets the
        /// memory-backed tests exercise the durability gate of a checkpoint
        /// without a real RocksDB directory.
        fn with_simulated_durability(
            inner: Arc<dyn Store>,
            observations: Arc<Mutex<Vec<FlushObservation>>>,
        ) -> Self {
            Self {
                delegates_flush: false,
                ..Self::new(inner, observations)
            }
        }

        /// Lets the wrapper read the durable watermark the service reports.
        fn watch(&self, service: &Arc<ApplicationLogsService>) {
            *self.service.lock().expect("service slot") = Some(Arc::downgrade(service));
        }

        /// Makes the next durable flush fail.
        fn arm(&self) {
            self.armed.store(true, Ordering::SeqCst);
        }

        /// Number of records currently in the wrapped logs database.
        ///
        /// The records use byte keys, so this reads through a snapshot, which is
        /// also the path ApplicationLogs itself reads.
        fn stored_records(&self) -> usize {
            self.inner
                .snapshot()
                .find(None::<&Vec<u8>>, SeekDirection::Forward)
                .count()
        }
    }

    // Byte-keyed reads always go through a snapshot: `Store` only promises the
    // `StorageKey` shaped read trait plus byte-keyed writes, and ApplicationLogs
    // reads its records through `store.snapshot()`. Delegating `snapshot()` to the
    // wrapped store therefore keeps that path intact without claiming a byte-keyed
    // read trait the wrapped `dyn Store` cannot provide.
    impl ReadOnlyStoreGeneric<StorageKey, StorageItem> for ObservedLogsStore {
        fn try_get(&self, key: &StorageKey) -> Option<StorageItem> {
            ReadOnlyStoreGeneric::<StorageKey, StorageItem>::try_get(&*self.inner, key)
        }

        fn find(
            &self,
            key_prefix: Option<&StorageKey>,
            direction: SeekDirection,
        ) -> Box<dyn Iterator<Item = (StorageKey, StorageItem)> + '_> {
            ReadOnlyStoreGeneric::<StorageKey, StorageItem>::find(
                &*self.inner,
                key_prefix,
                direction,
            )
        }
    }

    impl ReadOnlyStore for ObservedLogsStore {}

    impl WriteStore<Vec<u8>, Vec<u8>> for ObservedLogsStore {
        fn delete(&mut self, _key: Vec<u8>) -> Result<(), StorageError> {
            // ApplicationLogs only ever writes through a snapshot, so a direct
            // store write means the write path changed under this wrapper.
            Err(StorageError::invalid_operation(
                "observed logs store does not forward direct deletes",
            ))
        }

        fn put(&mut self, _key: Vec<u8>, _value: Vec<u8>) -> Result<(), StorageError> {
            Err(StorageError::invalid_operation(
                "observed logs store does not forward direct puts",
            ))
        }
    }

    impl Store for ObservedLogsStore {
        fn snapshot(&self) -> Arc<dyn StoreSnapshot> {
            self.inner.snapshot()
        }

        fn on_new_snapshot(&self, handler: OnNewSnapshotDelegate) {
            self.inner.on_new_snapshot(handler);
        }

        fn try_flush_durable(&self) -> StorageResult<()> {
            // Count through a snapshot because the records use byte keys; the
            // snapshot sees exactly what the wrapped store would have written so far.
            let stored_records = self.stored_records();
            let watermark = self
                .service
                .lock()
                .expect("service slot")
                .as_ref()
                .and_then(std::sync::Weak::upgrade)
                .and_then(|logs| logs.watermark());
            self.observations
                .lock()
                .expect("flush observations")
                .push(FlushObservation {
                    stored_records,
                    watermark,
                });
            if self.armed.load(Ordering::SeqCst) {
                return Err(StorageError::CommitFailed(
                    "injected application logs durable flush failure".to_string(),
                ));
            }
            if self.delegates_flush {
                return self.inner.try_flush_durable();
            }
            Ok(())
        }

        fn as_any(&self) -> &dyn std::any::Any {
            self
        }
    }

    /// A RocksDB-backed node with ApplicationLogs registered.
    ///
    /// The importer only reports completion after a *fallible* durable flush,
    /// which a memory store does not implement, so every end-to-end import test
    /// needs a real backend. ApplicationLogs is registered before genesis is
    /// persisted, exactly like the node bootstrap does, so the log stream of an
    /// import covers every height it also covers in production.
    struct TestNode {
        chain_path: String,
        system: Arc<NeoSystem>,
        chain_store: Arc<dyn Store>,
        logs_store: Arc<dyn Store>,
        logs: Arc<ApplicationLogsService>,
    }

    impl TestNode {
        fn open(root: &Path) -> Self {
            Self::open_with_logs_store(root, |store| store)
        }

        /// Opens a node whose application-logs store is wrapped by `decorate`.
        ///
        /// The wrapper is how a test observes, or breaks, the logs durability gate
        /// of a checkpoint: the chain store stays a real RocksDB so its own
        /// fallible flush still succeeds.
        fn open_with_logs_store(
            root: &Path,
            decorate: impl FnOnce(Arc<dyn Store>) -> Arc<dyn Store>,
        ) -> Self {
            let settings = neo_protocol::ProtocolSettings::testnet();
            let chain_dir = root.join("chain");
            let logs_dir = root.join("application-logs");
            let chain_path = chain_dir.to_str().expect("ASCII chain path").to_string();
            let logs_path = logs_dir
                .to_str()
                .expect("ASCII application logs path")
                .to_string();

            let provider: Arc<dyn StoreProvider> =
                Arc::new(RocksDBStoreProvider::new(StorageConfig {
                    path: chain_dir,
                    read_only: false,
                    ..Default::default()
                }));
            // The state service keeps its trie and root index in the same store
            // the importer writes, so the differential storage comparison below
            // covers the state roots as well.
            let state_settings = neo_system::state_service::StateServiceSettings {
                full_state: false,
                path: String::new(),
                network: settings.network,
                auto_verify: false,
                max_find_result_items: 100,
                exception_policy: neo_primitives::UnhandledExceptionPolicy::Ignore,
            };
            let system = NeoSystem::new_with_state_service_deferring_genesis(
                settings.clone(),
                Some(provider),
                Some(chain_path.clone()),
                Some(state_settings),
            )
            .map_err(|err| anyhow::anyhow!(err.to_string()))
            .expect("rocksdb-backed neo system");

            let raw_logs_store = system
                .store_provider()
                .get_store(&logs_path)
                .map_err(|err| anyhow::anyhow!(err.to_string()))
                .expect("application logs store");
            let logs_store = decorate(raw_logs_store);
            let logs = Arc::new(ApplicationLogsService::new(
                neo_rpc::application_logs::ApplicationLogsSettings {
                    enabled: true,
                    network: settings.network,
                    path: logs_path,
                    ..Default::default()
                },
                Arc::clone(&logs_store),
            ));
            system
                .add_service::<ApplicationLogsService, _>(Arc::clone(&logs))
                .expect("register ApplicationLogs service");
            system
                .register_committing_handler(
                    Arc::clone(&logs) as Arc<dyn CommittingHandler + Send + Sync>
                )
                .expect("register committing handler");
            system
                .register_committed_handler(
                    Arc::clone(&logs) as Arc<dyn CommittedHandler + Send + Sync>
                )
                .expect("register committed handler");
            system
                .persist_genesis_if_absent()
                .map_err(|err| anyhow::anyhow!(err.to_string()))
                .expect("persist genesis");
            // Genesis records must be durable before the importer starts, exactly
            // as the node bootstrap leaves them. A wrapper that is armed to fail
            // only does so from the first checkpoint on, so this flush still runs.
            logs.try_flush_durable()
                .expect("genesis application logs flush");

            Self {
                chain_path,
                chain_store: system.store(),
                system,
                logs_store,
                logs,
            }
        }

        /// Hash of the tip an archive record must name as its predecessor.
        fn tip_hash(&self) -> UInt256 {
            read_local_checkpoint(&self.system).hash
        }

        fn import(
            &self,
            path: &Path,
            stop_height: Option<u32>,
            policy: ImportDrainPolicy,
        ) -> Result<ImportSummary> {
            import_acc_file_with_options(
                &self.system,
                path,
                Some(&self.chain_path),
                stop_height,
                Some(policy),
            )
        }

        fn chain_dump(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
            dump_store(&self.chain_store)
        }

        fn logs_dump(&self) -> Vec<(Vec<u8>, Vec<u8>)> {
            dump_store(&self.logs_store)
        }

        async fn close(self) {
            let Self {
                system,
                chain_store,
                logs_store,
                logs,
                ..
            } = self;
            // The logs service owns a handle to the logs store, so it must go
            // before the store itself or a reopen keeps the backend locked.
            drop(logs);
            Arc::clone(&system).shutdown().await.expect("shutdown");
            // Every handle must be gone before a test reopens the same paths.
            drop(chain_store);
            drop(logs_store);
            drop(system);
        }
    }

    fn dump_store(store: &Arc<dyn Store>) -> Vec<(Vec<u8>, Vec<u8>)> {
        let snapshot = store.snapshot();
        snapshot.find(None, SeekDirection::Forward).collect()
    }

    /// Builds an empty block on top of `prev_hash`.
    fn empty_block(index: u32, prev_hash: UInt256) -> Block {
        let mut header = Header::new();
        header.set_index(index);
        header.set_prev_hash(prev_hash);
        header.set_next_consensus(UInt160::zero());
        header.set_timestamp(u64::from(index));
        header.witness = Witness::new();
        Block {
            header,
            transactions: Vec::new(),
        }
    }

    /// Builds `count` linked empty blocks starting at `first_index`.
    fn empty_block_chain(first_index: u32, count: u32, first_prev: UInt256) -> Vec<Block> {
        let mut blocks = Vec::with_capacity(count as usize);
        let mut prev_hash = first_prev;
        for item in 0..count {
            let mut block = empty_block(first_index + item, prev_hash);
            prev_hash = block.hash();
            blocks.push(block);
        }
        blocks
    }

    /// Writes an `.acc` archive holding exactly `blocks`.
    fn write_archive(path: &Path, first_index: u32, blocks: &[Block]) {
        let mut acc = File::create(path).expect("create ACC");
        acc.write_all(&first_index.to_le_bytes())
            .expect("ACC start");
        acc.write_all(&(blocks.len() as u32).to_le_bytes())
            .expect("ACC count");
        for block in blocks {
            let mut writer = neo_io::BinaryWriter::new();
            block.serialize(&mut writer).expect("serialize block");
            let payload = writer.into_bytes();
            acc.write_all(&(payload.len() as u32).to_le_bytes())
                .expect("block length");
            acc.write_all(&payload).expect("block payload");
        }
    }

    /// Writes a linked run of empty blocks on top of the node tip.
    fn write_empty_archive(path: &Path, node: &TestNode, count: u32) {
        let blocks = empty_block_chain(1, count, node.tip_hash());
        write_archive(path, 1, &blocks);
    }

    fn test_policy(empty_batch_cap: usize, flush_interval: u64) -> ImportDrainPolicy {
        // Progress windows are pushed out of reach so a run is only split by the
        // boundary under test.
        ImportDrainPolicy::new(empty_batch_cap, flush_interval, 100_000, 60)
    }

    /// A batched import must produce exactly the storage, state roots, receipts
    /// and application logs that the same blocks produce when persisted alone.
    #[tokio::test(flavor = "multi_thread")]
    async fn batched_empty_import_matches_single_block_import() {
        let single_root = tempfile::tempdir().expect("temporary directory");
        let single = TestNode::open(single_root.path());
        let single_acc = single_root.path().join("chain.0.acc");
        write_empty_archive(&single_acc, &single, 7);
        let single_summary = single
            .import(&single_acc, None, test_policy(0, 10_000))
            .expect("single-block import");
        let single_storage = single.chain_dump();
        let single_logs = single.logs_dump();
        single.close().await;

        let batched_root = tempfile::tempdir().expect("temporary directory");
        let batched = TestNode::open(batched_root.path());
        let batched_acc = batched_root.path().join("chain.0.acc");
        write_empty_archive(&batched_acc, &batched, 7);
        let batched_summary = batched
            .import(&batched_acc, None, test_policy(3, 10_000))
            .expect("batched import");
        let batched_storage = batched.chain_dump();
        let batched_logs = batched.logs_dump();
        batched.close().await;

        assert_eq!(single_summary.imported, 7);
        assert_eq!(single_summary.counters.empty_block_batches, 0);
        assert_eq!(single_summary.counters.batched_empty_blocks, 0);
        assert_eq!(batched_summary.imported, 7);
        assert_eq!(batched_summary.final_height, single_summary.final_height);
        assert_eq!(batched_summary.counters.empty_blocks, 7);
        // Seven blocks with a cap of three drain as 3 + 3 + 1, so only the two
        // multi-block write-backs count as batches.
        assert_eq!(batched_summary.counters.empty_block_batches, 2);
        assert_eq!(batched_summary.counters.batched_empty_blocks, 6);

        assert_eq!(
            single_storage, batched_storage,
            "a batched import must leave byte-identical chain and state storage"
        );
        // Guard against a comparison that passes because both dumps are empty.
        assert!(
            single_storage.len() > 20,
            "the chain store must hold the imported chain, got {} keys",
            single_storage.len()
        );
        assert!(
            !single_logs.is_empty(),
            "the run must produce application logs to compare"
        );
        assert_eq!(
            single_logs, batched_logs,
            "a batched import must leave byte-identical application logs"
        );
        assert!(
            single_logs.len() > 7,
            "every imported height plus genesis must have a log record"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_run_is_persisted_exactly_at_the_cap() {
        let exact_root = tempfile::tempdir().expect("temporary directory");
        let exact = TestNode::open(exact_root.path());
        let exact_acc = exact_root.path().join("chain.0.acc");
        write_empty_archive(&exact_acc, &exact, 4);
        let exact_summary = exact
            .import(&exact_acc, None, test_policy(4, 10_000))
            .expect("import at the cap");
        exact.close().await;

        assert_eq!(exact_summary.imported, 4);
        assert_eq!(exact_summary.counters.empty_block_batches, 1);
        assert_eq!(exact_summary.counters.batched_empty_blocks, 4);

        // One block beyond the cap splits into a full batch and a single block.
        let overflow_root = tempfile::tempdir().expect("temporary directory");
        let overflow = TestNode::open(overflow_root.path());
        let overflow_acc = overflow_root.path().join("chain.0.acc");
        write_empty_archive(&overflow_acc, &overflow, 5);
        let overflow_summary = overflow
            .import(&overflow_acc, None, test_policy(4, 10_000))
            .expect("import past the cap");
        overflow.close().await;

        assert_eq!(overflow_summary.imported, 5);
        assert_eq!(overflow_summary.counters.empty_block_batches, 1);
        assert_eq!(overflow_summary.counters.batched_empty_blocks, 4);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn checkpoint_boundary_splits_a_run_below_the_cap() {
        let root = tempfile::tempdir().expect("temporary directory");
        let node = TestNode::open(root.path());
        let acc = root.path().join("chain.0.acc");
        write_empty_archive(&acc, &node, 7);

        // A cap of four would carry all seven blocks in two write-backs
        // (4 + 3). The checkpoint interval of three is what makes the run end at
        // three: a checkpoint is never crossed by a batch.
        let summary = node
            .import(&acc, None, test_policy(4, 3))
            .expect("import across checkpoints");
        node.close().await;

        assert_eq!(summary.imported, 7);
        assert_eq!(summary.counters.empty_block_batches, 2);
        assert_eq!(summary.counters.batched_empty_blocks, 6);
        assert_eq!(summary.final_height, 7);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn stop_height_inside_a_run_drains_only_up_to_the_limit() {
        let root = tempfile::tempdir().expect("temporary directory");
        let node = TestNode::open(root.path());
        let acc = root.path().join("chain.0.acc");
        write_empty_archive(&acc, &node, 6);

        // A cap of six would persist the whole archive in one write-back; the
        // stop height truncates the run at the third block instead.
        let summary = node
            .import(&acc, Some(3), test_policy(6, 10_000))
            .expect("import up to the stop height");
        let tip = node.tip_hash();
        node.close().await;

        assert_eq!(summary.imported, 3);
        assert_eq!(summary.final_height, 3);
        assert_eq!(summary.counters.empty_block_batches, 1);
        assert_eq!(summary.counters.batched_empty_blocks, 3);
        assert_ne!(tip, UInt256::default());
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_run_that_breaks_the_predecessor_chain_is_rejected_before_persistence() {
        let root = tempfile::tempdir().expect("temporary directory");
        let node = TestNode::open(root.path());
        let acc = root.path().join("chain.0.acc");
        let mut blocks = empty_block_chain(1, 4, node.tip_hash());
        // Block two now names a predecessor that is not block one, so the run is
        // not a linked chain even though every height is present.
        blocks[1].header.set_prev_hash(hash_of(0xEE));
        write_archive(&acc, 1, &blocks);

        let registers_before = node.logs_dump().len();
        let error = node
            .import(&acc, None, test_policy(4, 10_000))
            .expect_err("a broken predecessor chain must be rejected");
        let poisoned = node.system.is_persistence_poisoned();
        let height = node.system.current_block_index();
        let registers_after = node.logs_dump().len();
        node.close().await;

        // `{:#}` keeps the whole anyhow context chain, which is where the
        // rejected predecessor link is reported.
        let message = format!("{error:#}");
        assert!(
            message.contains("does not extend"),
            "unexpected error: {message}"
        );
        assert!(
            !poisoned,
            "a preflight rejection must not poison the system"
        );
        assert_eq!(height, 0, "no block of the rejected run may be accepted");
        assert!(
            registers_before > 0,
            "genesis must already have produced an application log"
        );
        assert_eq!(
            registers_after, registers_before,
            "a rejected run must not add application logs to the ones genesis produced"
        );
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn resume_after_a_batched_import_skips_every_persisted_height() {
        let root = tempfile::tempdir().expect("temporary directory");
        let acc = root.path().join("chain.0.acc");

        let first = TestNode::open(root.path());
        write_empty_archive(&acc, &first, 6);
        let first_summary = first
            .import(&acc, None, test_policy(3, 10_000))
            .expect("batched import");
        let first_storage = first.chain_dump();
        let first_logs = first.logs_dump();
        first.close().await;

        // Reopening the same paths is what a restart does: the importer must see
        // the batched tip and skip exactly the heights it already persisted.
        let second = TestNode::open(root.path());
        assert_eq!(second.system.current_block_index(), 6);
        let second_summary = second
            .import(&acc, None, test_policy(3, 10_000))
            .expect("resumed import");
        let second_storage = second.chain_dump();
        let second_logs = second.logs_dump();
        second.close().await;

        assert_eq!(first_summary.imported, 6);
        assert_eq!(first_summary.counters.empty_block_batches, 2);
        assert_eq!(second_summary.imported, 0);
        assert_eq!(second_summary.skipped, 6);
        assert_eq!(second_summary.final_height, 6);
        assert_eq!(second_summary.counters.empty_block_batches, 0);
        assert_eq!(
            first_storage, second_storage,
            "a resumed import must not rewrite the persisted chain"
        );
        assert_eq!(
            first_logs, second_logs,
            "a resumed import must not add application logs"
        );
    }

    #[tokio::test]
    async fn logs_unit_is_a_no_op_without_a_registered_logs_service() {
        let system = memory_system();
        let unit = LogsUnit::begin(&system).expect("open a unit without a logs service");
        unit.finish().expect("finish a unit without a logs service");
        LogsUnit::begin(&system)
            .expect("open a unit without a logs service")
            .abort();
    }

    /// An ApplicationLogs service attached to a system for checkpoint tests.
    struct AttachedLogs {
        logs: Arc<ApplicationLogsService>,
        store: Arc<ObservedLogsStore>,
    }

    /// Registers an ApplicationLogs service on `system` and returns it with the
    /// memory store its commits land in.
    ///
    /// The store is wrapped so its durable flush is fallible exactly as a RocksDB
    /// logs store is, and so the flush itself can be observed or armed to fail.
    fn attach_logs_service(system: &Arc<NeoSystem>) -> AttachedLogs {
        let store = Arc::new(ObservedLogsStore::with_simulated_durability(
            Arc::new(neo_storage::persistence::providers::MemoryStore::new()),
            Arc::new(Mutex::new(Vec::new())),
        ));
        let logs = Arc::new(ApplicationLogsService::new(
            neo_rpc::application_logs::ApplicationLogsSettings {
                enabled: true,
                network: system.settings().network,
                ..Default::default()
            },
            Arc::clone(&store) as Arc<dyn Store>,
        ));
        system
            .add_service::<ApplicationLogsService, _>(Arc::clone(&logs))
            .expect("register ApplicationLogs service");
        store.watch(&logs);
        AttachedLogs { logs, store }
    }

    /// Stages one block's records inside the open unit exactly the way the
    /// committing and committed callbacks do, including the boundary the
    /// watermark is taken from.
    fn stage_block(logs: &ApplicationLogsService, system: &Arc<NeoSystem>, block: &Block) {
        let execution = neo_protocol::execution::Block::from(block);
        let snapshot = neo_storage::persistence::PersistenceDataCache::new(false);
        logs.blockchain_committing_handler(system.as_ref(), &execution, &snapshot, &[]);
        logs.blockchain_committed_handler(system.as_ref(), &execution);
    }

    #[tokio::test]
    async fn logs_watermark_verification_rejects_records_without_a_watermark() {
        let system = memory_system();
        let logs = attach_logs_service(&system).logs;

        let error = verify_logs_watermark(&logs, 1, &hash_of(0x01), 3, "unit-test")
            .expect_err("committed records without a watermark must not verify");
        assert!(
            error
                .to_string()
                .contains("without a durable block watermark"),
            "unexpected error: {error}"
        );

        // Nothing committed is not a failure: a plugin that stopped on its
        // exception policy leaves exactly this state and must not fail an import.
        verify_logs_watermark(&logs, 1, &hash_of(0x01), 0, "unit-test")
            .expect("an empty flush needs no watermark");
    }

    #[tokio::test]
    async fn logs_watermark_verification_rejects_a_watermark_ahead_of_the_chain() {
        let system = memory_system();
        let logs = attach_logs_service(&system).logs;
        let session = LogsSession::begin(&system, 0, &UInt256::default()).expect("open session");

        let mut block = empty_block(9, hash_of(0x09));
        let block_hash = block.hash();
        assert_eq!(
            logs.begin_persistence_unit(),
            PersistenceUnitOutcome::Started
        );
        stage_block(&logs, &system, &block);
        logs.finish_persistence_unit().expect("finish unit");
        logs.flush_accepted().expect("flush accepted");
        assert_eq!(
            logs.watermark(),
            None,
            "a commit alone must not certify durability"
        );
        logs.try_flush_durable().expect("durable logs flush");
        assert_eq!(logs.watermark(), Some((9, block_hash)));

        let error = verify_logs_watermark(&logs, 8, &hash_of(0x08), 0, "unit-test")
            .expect_err("logs ahead of the chain must not verify");
        assert!(
            error
                .to_string()
                .contains("is ahead of the checkpoint height"),
            "unexpected error: {error}"
        );

        // The same watermark matches the checkpoint it actually describes.
        verify_logs_watermark(&logs, 9, &block_hash, 1, "unit-test")
            .expect("the flushed watermark must match its own checkpoint");
        session.end().expect("end session");
    }

    /// The checkpoint flushes the chain first, so a chain flush that cannot
    /// become durable must leave the logs exactly where they were.
    #[tokio::test]
    async fn a_failed_chain_flush_leaves_the_logs_watermark_unmoved() {
        let system = memory_system();
        let attached = attach_logs_service(&system);
        let logs = &attached.logs;
        let session = LogsSession::begin(&system, 0, &UInt256::default()).expect("open session");
        assert!(logs.import_session_active());

        let mut block = empty_block(1, hash_of(0x01));
        assert_eq!(
            logs.begin_persistence_unit(),
            PersistenceUnitOutcome::Started
        );
        stage_block(logs, &system, &block);
        logs.finish_persistence_unit().expect("finish unit");
        assert_eq!(logs.flush_telemetry().pending_records, 1);

        let checkpoint = read_local_checkpoint(&system);
        let error = flush_and_verify_checkpoint(
            &system,
            None,
            checkpoint.height,
            &checkpoint.hash,
            1,
            0,
            "ordering-test",
        )
        .expect_err("a memory store cannot report a durable chain flush");
        assert!(
            error
                .to_string()
                .contains("durable chain-store flush failed"),
            "unexpected error: {error}"
        );

        // Step two of the checkpoint is the first that can fail it, so the logs
        // never advanced: the record is still pending and nothing was written.
        assert_eq!(logs.watermark(), None);
        assert_eq!(logs.flush_telemetry().pending_records, 1);
        assert!(
            attached.store.stored_records() == 0,
            "a failed chain flush must not commit application logs"
        );

        // Closing the session still writes the record, so nothing was lost, but a
        // closing commit is not a durable flush: the watermark stays where the
        // last confirmed flush left it.
        let deltas = session.end().expect("end session");
        assert_eq!(deltas.flush_count, 1);
        assert_eq!(deltas.pending_records, 0);
        assert!(
            attached.store.stored_records() > 0,
            "the closing commit must write the record it inherited"
        );
        assert_eq!(
            logs.watermark(),
            None,
            "a closing commit without a durable flush must not certify a block"
        );
    }

    /// A logs durable flush that fails must fail the checkpoint instead of
    /// certifying it.
    ///
    /// The chain store is a real RocksDB, so its own fallible flush succeeds and
    /// the checkpoint really does reach the logs step. That is what makes the
    /// failure evidence about the logs gate rather than about the chain.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_failed_logs_durable_flush_fails_the_checkpoint() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store_slot: Arc<Mutex<Option<Arc<ObservedLogsStore>>>> = Arc::new(Mutex::new(None));
        let node = {
            let slot = Arc::clone(&store_slot);
            TestNode::open_with_logs_store(root.path(), move |inner| {
                let store = Arc::new(ObservedLogsStore::new(
                    inner,
                    Arc::new(Mutex::new(Vec::new())),
                ));
                *slot.lock().expect("store slot") = Some(Arc::clone(&store));
                store as Arc<dyn Store>
            })
        };
        let observed = store_slot
            .lock()
            .expect("store slot")
            .clone()
            .expect("observed logs store");
        observed.watch(&node.logs);
        // `open` flushed the genesis records itself; every later flush fails.
        observed.observations.lock().expect("observations").clear();
        observed.arm();

        let acc = root.path().join("chain.0.acc");
        write_empty_archive(&acc, &node, 2);
        let error = node
            .import(&acc, None, test_policy(1, 1))
            .expect_err("a logs durable flush failure must fail the checkpoint");
        let message = format!("{error:#}");
        let height = node.system.current_block_index();
        let watermark = node.logs.watermark();
        let observations =
            std::mem::take(&mut *observed.observations.lock().expect("observations"));
        drop(observed);
        node.close().await;

        assert!(
            message.contains("durable application-logs flush failed"),
            "unexpected error: {message}"
        );
        assert!(
            height >= 1,
            "the chain flush of the checkpoint still succeeded, so the logs step is what failed"
        );
        assert_eq!(
            watermark, None,
            "the watermark must not advance when the flush that would confirm it failed"
        );
        assert_eq!(
            observations.len(),
            2,
            "the checkpoint's flush plus the best-effort flush of the error path"
        );
        assert!(
            observations[0].stored_records > 0,
            "the records were committed before the flush that failed"
        );
        assert_eq!(
            observations[0].watermark, None,
            "the failing checkpoint flush must not see a promoted watermark"
        );
    }

    /// The checkpoint order is chain flush, logs commit, logs durable flush, then
    /// the watermark, and only then may it be reported successful.
    #[tokio::test(flavor = "multi_thread")]
    async fn checkpoint_promotes_the_logs_watermark_only_after_the_durable_flush() {
        let root = tempfile::tempdir().expect("temporary directory");
        let store_slot: Arc<Mutex<Option<Arc<ObservedLogsStore>>>> = Arc::new(Mutex::new(None));
        let node = {
            let slot = Arc::clone(&store_slot);
            TestNode::open_with_logs_store(root.path(), move |inner| {
                let store = Arc::new(ObservedLogsStore::new(
                    inner,
                    Arc::new(Mutex::new(Vec::new())),
                ));
                *slot.lock().expect("store slot") = Some(Arc::clone(&store));
                store as Arc<dyn Store>
            })
        };
        let observed = store_slot
            .lock()
            .expect("store slot")
            .clone()
            .expect("observed logs store");
        observed.watch(&node.logs);
        observed.observations.lock().expect("observations").clear();

        let acc = root.path().join("chain.0.acc");
        write_empty_archive(&acc, &node, 3);
        let summary = node
            .import(&acc, None, test_policy(1, 3))
            .expect("an import whose logs flush succeeds");
        let observations =
            std::mem::take(&mut *observed.observations.lock().expect("observations"));
        let watermark = node.logs.watermark();
        let tip = node.tip_hash();
        drop(observed);
        node.close().await;

        assert_eq!(
            summary.imported, 3,
            "success is reported only after the checkpoint gates passed"
        );
        assert_eq!(
            observations.len(),
            1,
            "the run is exactly one checkpoint interval"
        );
        assert!(
            observations[0].stored_records > 0,
            "the logs commit must precede the durable flush"
        );
        assert_eq!(
            observations[0].watermark, None,
            "the watermark must still be unadvanced while the flush runs"
        );
        assert_eq!(
            watermark,
            Some((3, tip)),
            "the watermark advances only once the flush succeeded"
        );
    }

    /// Ten blocks inside one checkpoint interval cost exactly one logs commit.
    #[tokio::test(flavor = "multi_thread")]
    async fn import_coalesces_application_logs_into_one_commit() {
        let root = tempfile::tempdir().expect("temporary directory");
        let node = TestNode::open(root.path());
        let acc = root.path().join("chain.0.acc");
        write_empty_archive(&acc, &node, 10);

        // The checkpoint interval reaches past the archive, so only the final
        // checkpoint commits and the session closes on an empty pending set.
        let summary = node
            .import(&acc, None, test_policy(0, 10_000))
            .expect("coalesced import");
        let logs = node.logs_dump();
        node.close().await;

        assert_eq!(summary.imported, 10);
        assert_eq!(
            summary.logs.flush_count, 1,
            "one checkpoint interval must cost one logs commit"
        );
        assert_eq!(summary.logs.flushed_records, 10);
        assert!(summary.logs.flushed_bytes > 0);
        assert_eq!(
            summary.logs.pending_records, 0,
            "closing the session must leave nothing pending"
        );
        assert!(
            logs.len() > 10,
            "genesis plus every imported height has a log record, got {}",
            logs.len()
        );
    }

    /// A checkpoint interval inside the archive commits once per interval, never
    /// once per block.
    #[tokio::test(flavor = "multi_thread")]
    async fn periodic_checkpoints_commit_application_logs_once_each() {
        let root = tempfile::tempdir().expect("temporary directory");
        let node = TestNode::open(root.path());
        let acc = root.path().join("chain.0.acc");
        write_empty_archive(&acc, &node, 9);

        let summary = node
            .import(&acc, None, test_policy(0, 3))
            .expect("import across checkpoints");
        node.close().await;

        assert_eq!(summary.imported, 9);
        assert_eq!(
            summary.logs.flush_count, 3,
            "three checkpoints must cost three logs commits"
        );
        assert_eq!(summary.logs.flushed_records, 9);
        assert_eq!(summary.logs.pending_records, 0);
    }
}
