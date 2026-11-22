use super::*;
impl StateServiceCommitHandlers {
    /// Indicates whether this handler should run during fast sync.
    ///
    /// State root computation should run during fast sync to maintain
    /// state root history for verification.
    pub fn run_during_fast_sync(&self) -> bool {
        self.state_root_enabled
    }

    /// Pre-commit hook: computes the state root from the snapshot before it is
    /// persisted to storage, then stages the local-root cursor.
    ///
    /// `context` is the runtime commit port; `None` retains the post-commit
    /// fallback for direct embedders without a system context.
    ///
    /// # Reference
    /// C# Neo StateService: Blockchain_Committing event handler
    pub fn blockchain_committing_handler(
        &self,
        context: Option<&dyn StateCommitContext>,
        block: &neo_protocol::execution::Block,
        snapshot: &PersistenceDataCache,
        _application_executed_list: &[neo_protocol::execution::ApplicationExecuted],
    ) {
        if !self.state_root_enabled {
            return;
        }

        let height = block.index();

        match self
            .commit_block_with_parallelism(height, snapshot, !block.transactions.is_empty())
            .and_then(|()| self.stage_local_root_progress(context, snapshot))
        {
            Ok(()) => {
                debug!(
                    target: "neo::state_service",
                    height,
                    "blockchain_committing: state root computed"
                );
            }
            Err(err) => {
                error!(
                    target: "neo::state_service",
                    height,
                    %err,
                    "blockchain_committing: failed to compute state root"
                );
            }
        }
    }

    /// Fallible committing hook for protocol-critical state root computation.
    ///
    /// Returns an error if state root computation fails, allowing the
    /// blockchain to handle the failure appropriately.
    pub fn try_blockchain_committing_handler(
        &self,
        context: Option<&dyn StateCommitContext>,
        block: &neo_protocol::execution::Block,
        snapshot: &PersistenceDataCache,
        _application_executed_list: &[neo_protocol::execution::ApplicationExecuted],
    ) -> Result<(), StateServiceError> {
        if !self.state_root_enabled {
            return Ok(());
        }

        let height = block.index();
        self.commit_block_with_parallelism(height, snapshot, !block.transactions.is_empty())
            .and_then(|()| self.stage_local_root_progress(context, snapshot))
            .map_err(|e| {
                error!(
                    target: "neo::state_service",
                    height,
                    error = %e,
                    "try_blockchain_committing_handler: state root computation failed"
                );
                e
            })
    }

    /// Rollback hook: discards the staged local-root marker and rolls back the
    /// incremental trie state after a failed participant vote or store commit.
    pub fn blockchain_rollback_handler(
        &self,
        context: Option<&dyn StateCommitContext>,
        block: &neo_protocol::execution::Block,
        _snapshot: &PersistenceDataCache,
    ) -> Result<(), StateServiceError> {
        self.discard_staged_local_root(context, block.index());
        self.rollback()
    }

    /// Post-commit hook: persists the state root computed during the committing
    /// phase and emits notification events.
    ///
    /// # Reference
    /// C# Neo StateService: Blockchain_Committed event handler
    /// - Persists state root to storage
    /// - Emits ValidatedRootPersisted event
    pub fn blockchain_committed_handler(
        &self,
        context: Option<&dyn StateCommitContext>,
        block: &neo_protocol::execution::Block,
    ) {
        if !self.state_root_enabled {
            return;
        }

        let index = block.index();

        match self.persist_pending_state_root(index) {
            Ok(Some(state_root)) => {
                self.announce_validated_root(context, index, &state_root);
                debug!(
                    target: "neo::state_service",
                    index,
                    "blockchain_committed: state root persisted and announced"
                );
            }
            Ok(None) => {
                debug!(
                    target: "neo::state_service",
                    index,
                    "blockchain_committed: no pending state root to persist"
                );
            }
            Err(err) => {
                error!(
                    target: "neo::state_service",
                    index,
                    %err,
                    "blockchain_committed: failed to persist state root"
                );
            }
        }
    }
}
