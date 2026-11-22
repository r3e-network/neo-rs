use super::ImportSummary;
use super::checkpoint::*;
use super::env::*;
use super::logs::*;
use super::run::*;
use crate::import_batching::{
    DrainTrigger, ImportDrainPolicy, MAX_EMPTY_BATCH_SIZE, clamp_empty_batch_size,
    empty_run_drain_trigger,
};
use crate::import_telemetry::{
    FastPathDeltas, ImportCounters, LogsDeltas, ParallelDeltas, PersistenceProfileDeltas,
    nanos_to_millis,
};
use anyhow::{Context, Result, bail};
use neo_rpc::application_logs::ApplicationLogsService;
use neo_system::{NeoSystem, PERSISTENCE_STAGE_NESTING};
use std::{
    fs::File,
    io::{BufReader, Read, Seek, SeekFrom},
    path::Path,
    sync::Arc,
    time::Instant,
};
use tracing::{info, warn};

/// Imports `.acc` records until `stop_height` (inclusive) is reached.
///
/// [`import_acc_file`] takes the limit from `NEO_IMPORT_STOP_HEIGHT`; the
/// explicit parameter keeps the boundary testable without mutating process-wide
/// environment variables.
pub(super) fn import_acc_file_until(
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
pub(super) fn import_acc_file_with_options(
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
