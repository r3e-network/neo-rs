use super::*;
/// Persists one canonical block through the pure 15-step pipeline.
///
/// See the module documentation for the immutable step order. This function performs no
/// I/O of its own; every effect flows through the supplied ports.
pub fn persist_block(
    block: Block,
    store: &dyn SnapshotStore,
    engines: &dyn EngineFactory,
    runtime_cache: &dyn RuntimeCacheSink,
    evidence: &dyn EvidenceSink,
    participants: &[Arc<dyn CommitParticipant>],
    options: &PersistOptions,
) -> LedgerResult<PersistReceipt> {
    persist_block_profiled(
        block,
        store,
        engines,
        runtime_cache,
        evidence,
        participants,
        options,
        None,
    )
}

/// Runs the canonical pipeline with optional observation-only stage timings.
#[allow(clippy::too_many_arguments)]
pub fn persist_block_profiled(
    block: Block,
    store: &dyn SnapshotStore,
    engines: &dyn EngineFactory,
    runtime_cache: &dyn RuntimeCacheSink,
    evidence: &dyn EvidenceSink,
    participants: &[Arc<dyn CommitParticipant>],
    options: &PersistOptions,
    profiler: Option<&dyn PersistenceProfiler>,
) -> LedgerResult<PersistReceipt> {
    persist_block_with_loop_strategy(
        block,
        store,
        engines,
        runtime_cache,
        evidence,
        participants,
        options,
        profiler,
        None,
    )
}

/// Runs the canonical pipeline with an alternative step 10 implementation.
///
/// `strategy: None` keeps [`execute_serially`] — the canonical serial loop, which is
/// what [`persist_block`] and [`persist_block_profiled`] always use. A supplied
/// strategy only replaces *where* the transactions execute; the surrounding fourteen
/// steps, including the store commit and the participant ordering, are untouched.
#[allow(clippy::too_many_arguments)]
pub fn persist_block_with_loop_strategy(
    block: Block,
    store: &dyn SnapshotStore,
    engines: &dyn EngineFactory,
    runtime_cache: &dyn RuntimeCacheSink,
    evidence: &dyn EvidenceSink,
    participants: &[Arc<dyn CommitParticipant>],
    options: &PersistOptions,
    profiler: Option<&dyn PersistenceProfiler>,
    strategy: Option<&dyn TransactionLoopStrategy>,
) -> LedgerResult<PersistReceipt> {
    // Step 1 — resolve options and detailed-execution gating.
    let fast_sync = options.fast_sync;
    let detailed = !fast_sync || participants.iter().any(|p| p.run_during_fast_sync());

    // Step 2 — canonical block identity + bounded fault-evidence buffer.
    let block_hash = block.hash();
    let block_height = block.index();
    let persisting_block = Arc::new(block);
    let fault_capacity = evidence.available_capacity();
    let mut fault_buffer = FaultObservationBuffer::for_available_capacity(fault_capacity);

    // Steps 3/4 — open the store write transaction and derive the base overlay snapshot.
    let transaction = {
        let _stage = StageTimer::new(profiler, PersistenceStage::SnapshotOpen);
        store.open_transaction(options)?
    };
    let base_snapshot = transaction.base_snapshot();

    // Step 5 — pre-compute transaction hashes (fail-closed before any engine work).
    // Hashing is pure and order-independent. Indexed parallel collection preserves
    // transaction order, including which error is returned first.
    use rayon::prelude::*;
    let transaction_hashes = {
        let _stage = StageTimer::new(profiler, PersistenceStage::TransactionHash);
        let hashes = if persisting_block.transactions.len() >= 2 {
            persisting_block
                .transactions
                .par_iter()
                .map(neo_protocol::payloads::Transaction::try_hash)
                .collect::<Vec<_>>()
        } else {
            persisting_block
                .transactions
                .iter()
                .map(neo_protocol::payloads::Transaction::try_hash)
                .collect::<Vec<_>>()
        };
        hashes
            .into_iter()
            .collect::<Result<Vec<_>, _>>()
            .map_err(|err| LedgerError::TransactionHash {
                message: err.to_string(),
            })?
    };

    // Steps 6/7 — OnPersist native execution and receipt.
    let mut receipts = Vec::with_capacity(persisting_block.transactions.len() + 2);
    let mut on_persist = {
        let _stage = StageTimer::new(profiler, PersistenceStage::OnPersist);
        let mut engine =
            engines.open_on_persist(Arc::clone(&base_snapshot), Arc::clone(&persisting_block))?;
        engine.native_on_persist()?;
        engine.record_native_persist_success();
        if detailed {
            receipts.push(engine.execution_receipt());
        }
        engine
    };

    // Step 8 — seed native-only contract states + shared native cache handle.
    let seeded = on_persist.seed_native_contracts();
    let native_cache = on_persist.native_cache_handle();

    // Step 9 — take the ledger transaction state token from the OnPersist engine.
    let mut tx_states: LedgerTransactionStates = on_persist.take_transaction_states();

    // Step 10 — per-transaction application loop.
    //
    // The canonical serial loop over one shared child overlay is [`execute_serially`];
    // a caller-supplied strategy may replace it. Either way the outcome is folded back
    // in block order so steps 11-15 see exactly what the serial loop would have produced.
    {
        let _stage = StageTimer::new(profiler, PersistenceStage::TransactionLoop);
        let context = TransactionLoopContext {
            block: &persisting_block,
            transaction_hashes: &transaction_hashes,
            base_snapshot: &base_snapshot,
            seeded: &seeded,
            native_cache: &native_cache,
            detailed,
            fault_capacity,
            profiler,
        };
        let outcome = match strategy {
            Some(strategy) => strategy.execute(engines, &context, tx_states)?,
            None => execute_serially(engines, &context, tx_states)?,
        };
        tx_states = outcome.transaction_states;
        receipts.extend(outcome.receipts);
        for observation in outcome.faults {
            fault_buffer.push(observation);
        }
        fault_buffer.dropped = fault_buffer.dropped.saturating_add(outcome.dropped_faults);
    }

    // Step 11 — PostPersist native execution over the merged base snapshot.
    {
        let _stage = StageTimer::new(profiler, PersistenceStage::PostPersist);
        let mut post_persist = engines.open_post_persist(
            Arc::clone(&base_snapshot),
            Arc::clone(&persisting_block),
            &seeded,
            &native_cache,
        )?;
        post_persist.set_transaction_states(tx_states);
        post_persist.native_post_persist()?;
        post_persist.record_native_persist_success();
        if detailed {
            receipts.push(post_persist.execution_receipt());
        }
    }

    // Step 12 — commit participants stage/precommit in registration order.
    let stage = StageContext {
        block: &persisting_block,
        snapshot: &base_snapshot,
        receipts: &receipts,
        fast_sync,
    };
    let mut staged: Vec<usize> = Vec::new();
    {
        let _stage = StageTimer::new(profiler, PersistenceStage::ParticipantPrecommit);
        for (index, participant) in participants.iter().enumerate() {
            if fast_sync && !participant.run_during_fast_sync() {
                continue;
            }
            // Record the attempt *before* the vote. A participant can write partial
            // staging state and then fail, so excluding it from the staging list
            // would leak that state: reverse discard below must include it.
            staged.push(index);
            if let Err(message) = participant.precommit(&stage) {
                discard_staged_reverse(participants, &staged, &stage);
                return Err(LedgerError::ParticipantCommit { message });
            }
        }
    }

    // Step 13 — canonical store commit.
    {
        let _stage = StageTimer::new(profiler, PersistenceStage::StoreCommit);
        if let Err(message) = transaction.commit() {
            discard_staged_reverse(participants, &staged, &stage);
            return Err(LedgerError::StoreCommit {
                index: block_height,
                message,
            });
        }
    }

    // Step 14 — runtime cache / tip update.
    //
    // The store commit above is already durable. A failure here can therefore only
    // make the *local runtime view* stale: it must not be reported as "nothing was
    // committed", must not discard staged participants (their state is part of the
    // durable commit) and must not attempt to roll back committed data.
    let cache_failure = {
        let _stage = StageTimer::new(profiler, PersistenceStage::RuntimeUpdate);
        if options.update_runtime_cache {
            runtime_cache
                .record_block(&persisting_block)
                .err()
                .map(|message| LedgerError::PostCommitCacheFailure {
                    index: block_height,
                    message,
                })
        } else {
            runtime_cache.record_tip(block_height);
            None
        }
    };

    if let Some(error) = &cache_failure {
        tracing::error!(
            target: "neo",
            index = block_height,
            %block_hash,
            error = %error,
            "CRITICAL: block is durably committed but the runtime cache update failed; \
             local tip/caches are stale until repaired by reload"
        );
    }

    let (staged_fault_observations, dropped) = {
        let _stage = StageTimer::new(profiler, PersistenceStage::PostCommit);
        // Step 15a — infallible committed participants (registration order).
        let commit_context = CommitContext {
            block: &persisting_block,
            fast_sync,
        };
        for participant in participants {
            participant.committed(&commit_context);
        }

        // Step 15b — bounded fault-evidence export (never fails the pipeline).
        let staged_fault_observations = fault_buffer.staged_len();
        let mut dropped = fault_buffer.dropped;
        for observation in std::mem::take(&mut fault_buffer.observations) {
            let emitted = evidence.try_emit(&observation);
            dropped = dropped.saturating_add(u64::from(!emitted));
        }
        evidence.record_dropped(dropped);

        // Step 15c — plugin BlockReceived broadcast (detailed only, never fails).
        if detailed {
            runtime_cache.broadcast_block_received(block_hash.to_string(), block_height);
        }
        (staged_fault_observations, dropped)
    };

    // Durable state advanced even when the runtime cache did not: report the
    // committed-but-stale outcome so the caller never retries the height.
    if let Some(error) = cache_failure {
        return Err(error);
    }

    Ok(PersistReceipt {
        block_hash,
        block_height,
        detailed,
        receipts,
        staged_fault_observations,
        dropped_fault_observations: dropped,
    })
}

/// Discards already-staged participants in reverse registration order (steps 12/13/14).
fn discard_staged_reverse(
    participants: &[Arc<dyn CommitParticipant>],
    staged: &[usize],
    context: &StageContext<'_>,
) {
    for &index in staged.iter().rev() {
        participants[index].discard(context);
    }
}
