use super::snapshot_store::SystemSnapshotStore;
use super::write_back::EmptyBlockProfileGuard;
use super::*;

impl NeoSystem {
    /// Persists a block through the configured execution and commit pipeline.
    pub fn persist_block(&self, block: Block) -> Result<(), String> {
        self.persist_block_internal(block, true, self.execution_mode())
            .map(|_| ())
    }

    /// Persists a block with an explicitly selected execution engine.
    pub fn persist_block_with_execution_mode(
        &self,
        block: Block,
        execution_mode: ExecutionMode,
    ) -> Result<(), String> {
        self.persist_block_internal(block, true, execution_mode)
            .map(|_| ())
    }

    /// Persists a block and returns the actual native and transaction execution receipts.
    pub fn persist_block_with_receipts(
        &self,
        block: Block,
    ) -> Result<Vec<ApplicationExecuted>, String> {
        self.persist_block_internal(block, true, self.execution_mode())
    }

    /// Persists with receipts and an explicitly selected execution engine.
    pub fn persist_block_with_receipts_and_execution_mode(
        &self,
        block: Block,
        execution_mode: ExecutionMode,
    ) -> Result<Vec<ApplicationExecuted>, String> {
        self.persist_block_internal(block, true, execution_mode)
    }

    /// Persists an imported block without retaining it in online runtime caches.
    pub fn persist_block_without_runtime_cache(
        &self,
        block: neo_protocol::payloads::Block,
    ) -> CoreResult<Vec<ApplicationExecuted>> {
        self.persist_block_without_runtime_cache_with_execution_mode(block, self.execution_mode())
    }

    /// Persists an imported block with an explicitly selected execution engine.
    pub fn persist_block_without_runtime_cache_with_execution_mode(
        &self,
        block: neo_protocol::payloads::Block,
        execution_mode: ExecutionMode,
    ) -> CoreResult<Vec<ApplicationExecuted>> {
        self.persist_block_internal(Block::from(&block), false, execution_mode)
            .map_err(CoreError::system)
    }

    /// Maximum number of logical blocks retained by one empty-block write-back cache.
    pub const MAX_EMPTY_BLOCK_WRITE_BACK_BLOCKS: usize = 4096;

    /// Persists a bounded run of transaction-free blocks through the canonical pipeline.
    ///
    /// Logical state, receipts and state roots are produced for every height, while
    /// physical writes are coalesced into one atomic store transaction. Any error
    /// requires discarding this `NeoSystem`: committed callbacks from earlier logical
    /// blocks may already have advanced process-local state even though the batch's
    /// physical transaction was not committed.
    ///
    /// This method enforces that requirement itself instead of leaving it to callers.
    /// Once a batch has started, the first failure poisons this instance: the
    /// process-local tip, state root and callback state may be ahead of durable
    /// storage, so every later persistence call (including this one) is rejected with
    /// a poison error until the instance is dropped and storage is reopened.
    pub fn persist_consecutive_empty_blocks_without_runtime_cache(
        &self,
        mut blocks: Vec<neo_protocol::payloads::Block>,
    ) -> Result<Vec<Vec<ApplicationExecuted>>, String> {
        self.ensure_persistence_live()?;
        if !(2..=Self::MAX_EMPTY_BLOCK_WRITE_BACK_BLOCKS).contains(&blocks.len()) {
            return Err(format!(
                "empty-block write-back requires 2..={} blocks",
                Self::MAX_EMPTY_BLOCK_WRITE_BACK_BLOCKS
            ));
        }
        if blocks.iter().any(|block| !block.transactions.is_empty()) {
            return Err("empty-block write-back rejected a transaction-bearing block".to_string());
        }
        let mut previous: Option<(u32, neo_primitives::UInt256)> = None;
        for block in &mut blocks {
            if let Some((previous_index, previous_hash)) = previous
                && (block.index() != previous_index + 1 || block.prev_hash() != &previous_hash)
            {
                return Err("empty-block write-back requires contiguous block hashes".to_string());
            }
            previous = Some((block.index(), block.hash()));
        }

        let commit_profiler = self
            .execution_stats
            .profiling_enabled
            .load(Ordering::Relaxed)
            .then(|| Arc::clone(&self.execution_stats) as Arc<dyn StoreCommitProfiler>);
        let store =
            SystemSnapshotStore::for_empty_block_batch(self.store(), commit_profiler, blocks.len());
        let execution_mode = self.execution_mode();
        // Every step below may advance process-local state before the batch's
        // physical commit, so any failure must poison this instance.
        let result = (|| {
            let mut executions = Vec::with_capacity(blocks.len());
            for block in blocks {
                executions.push(self.persist_block_with_store(
                    Block::from(&block),
                    false,
                    execution_mode,
                    &store,
                )?);
            }
            store.finish_empty_block_batch()?;
            Ok(executions)
        })();
        if result.is_err() {
            self.poison_persistence();
        }
        result
    }

    fn persist_block_internal(
        &self,
        block: Block,
        update_runtime_cache: bool,
        execution_mode: ExecutionMode,
    ) -> Result<Vec<ApplicationExecuted>, String> {
        self.ensure_persistence_live()?;
        let commit_profiler = self
            .execution_stats
            .profiling_enabled
            .load(Ordering::Relaxed)
            .then(|| Arc::clone(&self.execution_stats) as Arc<dyn StoreCommitProfiler>);
        let store = SystemSnapshotStore::new(self.store(), commit_profiler);
        self.persist_block_with_store(block, update_runtime_cache, execution_mode, &store)
    }

    fn persist_block_with_store(
        &self,
        block: Block,
        update_runtime_cache: bool,
        execution_mode: ExecutionMode,
        store: &SystemSnapshotStore,
    ) -> Result<Vec<ApplicationExecuted>, String> {
        let context = self.context();
        let recorder = Arc::new(ExecutionRecorder::with_event_stream(context.event_stream()));
        let engines = SystemEngineFactory {
            settings: self.settings().clone(),
            native_services: self.native_services.clone(),
            native_method_cache: self.native_method_cache_handle(),
            recorder: Arc::clone(&recorder),
            execution_mode,
            stats: Arc::clone(&self.execution_stats),
            payload_block: std::sync::OnceLock::new(),
        };
        let runtime_cache = SystemRuntimeCache {
            context: Arc::clone(&context),
        };
        let evidence = SystemEvidenceSink {
            context: Arc::clone(&context),
        };
        let participants = self.commit_participants(Arc::clone(&recorder))?;
        let options = PersistOptions {
            fast_sync: context.is_fast_sync_mode(),
            update_runtime_cache,
        };

        let profiler = self
            .execution_stats
            .profiling_enabled
            .load(Ordering::Relaxed)
            .then_some(self.execution_stats.as_ref() as &dyn PersistenceProfiler);
        let _empty_block_profile =
            profiler.map(|_| EmptyBlockProfileGuard::new(block.transactions.is_empty()));
        let parallel_loop = self
            .is_parallel_execution_enabled()
            .then(|| super::super::ParallelTransactionLoop::new(Arc::clone(&self.execution_stats)));
        neo_ledger::persist_block_with_loop_strategy(
            block,
            store,
            &engines,
            &runtime_cache,
            &evidence,
            &participants,
            &options,
            profiler,
            parallel_loop
                .as_ref()
                .map(|scheduler| scheduler as &dyn TransactionLoopStrategy),
        )
        .map_err(|error| {
            // C# v3.11.2 rethrows `UnknownHardforkException` out of
            // `Blockchain.Persist`, halting the node. Poison this instance so
            // later persistence calls fail fast instead of following a chain
            // this build cannot represent.
            if error.is_unknown_hardfork_fault() {
                tracing::error!("unknown hardfork fault halts block persistence: {error}");
                self.poison_persistence();
            }
            error.to_string()
        })?;
        Ok(recorder.snapshot())
    }

    fn commit_participants(
        &self,
        recorder: Arc<ExecutionRecorder>,
    ) -> Result<Vec<Arc<dyn CommitParticipant>>, String> {
        let system = self
            .self_ref
            .lock()
            .upgrade()
            .ok_or_else(|| "NeoSystem is not available for block persistence".to_string())?;
        let context = self.context();
        let committing = { context.committing_handlers.read().clone() };
        let committed = { context.committed_handlers.read().clone() };
        let mut participants = Vec::with_capacity(committing.len() + committed.len());

        for handler in committing {
            let run_during_fast_sync = handler.run_during_fast_sync();
            let system_for_commit = Arc::clone(&system);
            let system_for_rollback = Arc::clone(&system);
            let rollback_handler = Arc::clone(&handler);
            let recorder_for_commit = Arc::clone(&recorder);
            let participant = HandlerParticipantBridge::new(
                move |stage| {
                    let executed = recorder_for_commit.snapshot();
                    handler
                        .try_blockchain_committing_handler(
                            system_for_commit.as_ref(),
                            stage.block,
                            stage.snapshot,
                            &executed,
                        )
                        .map_err(|error| error.to_string())
                },
                |_| {},
            )
            .with_rollback(move |stage| {
                if let Err(error) = rollback_handler.blockchain_rollback_handler(
                    system_for_rollback.as_ref(),
                    stage.block,
                    stage.snapshot,
                ) {
                    tracing::warn!(
                        target: "neo::persistence",
                        index = stage.block.index(),
                        %error,
                        "commit participant rollback failed"
                    );
                }
            })
            .with_fast_sync(run_during_fast_sync);
            participants.push(Arc::new(participant) as Arc<dyn CommitParticipant>);
        }

        for handler in committed {
            let system_for_commit = Arc::clone(&system);
            let participant = HandlerParticipantBridge::new(
                |_| Ok(()),
                move |commit| {
                    handler.blockchain_committed_handler(system_for_commit.as_ref(), commit.block);
                },
            );
            participants.push(Arc::new(participant) as Arc<dyn CommitParticipant>);
        }

        Ok(participants)
    }
}

/// Attaches or detaches the state-root timing sink that feeds MPT stage timings
/// into [`ExecutionStats`].
///
/// Observation only: the sink copies durations the commit pipeline already
/// measured and the statistics only record them while profiling is enabled.
/// Installing or removing it cannot change an execution result, consensus
/// decision, or commit ordering.
pub(crate) fn install_state_root_profile_sink(
    handlers: &crate::state_service::StateServiceCommitHandlers,
    stats: &Arc<ExecutionStats>,
    enabled: bool,
) {
    if !enabled {
        handlers.set_profile_sink(None);
        return;
    }
    let stats = Arc::clone(stats);
    let sink: crate::state_service::StateRootProfileSink = Arc::new(move |profile| {
        stats.record_state_root_profile(profile.change_extraction, profile.mpt, profile.mpt_detail);
    });
    handlers.set_profile_sink(Some(sink));
}
