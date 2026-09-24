#![allow(missing_docs, dead_code)] // WIP integration; docs pending cleanup
//! Integration Hooks for Optimistic Parallel Execution
//!
//! This module provides integration points between the optimistic executor
//! and existing block processing loops. It demonstrates how to swap
//! serial execution for parallel execution without breaking changes.

use crate::state_service::optimistic_executor::{
    OptimisticExecutor, OptimisticExecutorConfig, OptimisticExecutorError, BlockExecutionOutcome,
    ParallelTxResult,
};
use crate::state_service::conflict_detector::ConflictResolution;
use crate::state_service::prefetch_pipeline::CoreResult;
use crate::persistence::DataCache;
use neo_vm::RwSetTracker;
use crate::Block;

/// Optional integration hook - replaces serial tx execution with parallel version
pub struct OptimisticExecutionHook {
    /// Executor instance (optional - allows toggle)
    executor: Option<OptimisticExecutor>,

    /// Configuration
    config: OptimisticExecutorConfig,
}

impl OptimisticExecutionHook {
    /// Creates new hook with default configuration
    #[must_use]
    pub fn new() -> Self {
        Self {
            executor: None,
            config: OptimisticExecutorConfig::default(),
        }
    }

    /// Enables optimistic parallel execution
    pub fn enable(&mut self) -> anyhow::Result<()> {
        self.config.enabled = true;
        
        let mut executor = OptimisticExecutor::new(self.config.clone());
        executor.initialize()?;
        
        self.executor = Some(executor);
        Ok(())
    }

    /// Disables parallel mode, falls back to serial
    pub fn disable(&mut self) {
        self.config.enabled = false;
        self.executor = None;
    }

    /// Executes block using parallel or serial method based on configuration.
    /// The serial fallback currently faults; callers need an executor to use
    /// parallel mode. The example checks API usage only.
    ///
    /// ```rust,no_run
    /// use neo_core::{Block, CoreError, persistence::DataCache};
    /// use neo_core::state_service::parallel_execution_integration::OptimisticExecutionHook;
    ///
    /// async fn example(
    ///     hook: &OptimisticExecutionHook,
    ///     block: &Block,
    ///     state: &DataCache,
    /// ) -> Result<(), CoreError> {
    ///     hook.execute_block(block, state).await?;
    ///     Ok(())
    /// }
    /// ```
    pub async fn execute_block(
        &self,
        block: &Block,
        initial_state: &DataCache,
    ) -> CoreResult<BlockExecutionOutcome> {
        if let Some(ref executor) = self.executor {
            // Parallel mode
            executor.execute_block_optimistically(block, initial_state)
                .await
                .map_err(|e| crate::CoreError::system(e.to_string()))
        } else {
            // Fallback to serial - caller handles this
            Err(crate::CoreError::system(
                "Serial execution not implemented in optimizer_hook".to_string()
            ))
        }
    }
}

impl Default for OptimisticExecutionHook {
    fn default() -> Self {
        Self::new()
    }
}

/// Alternative approach: Create executor inline during block execution
pub async fn execute_optimistically_inline(
    block: &Block,
    snapshot: DataCache,
) -> Result<(Vec<ParallelTxResult>, ConflictResolution), OptimisticExecutorError> {
    // Quick initialization of executor just for this block
    let mut executor = OptimisticExecutor::new(OptimisticExecutorConfig::default());
    executor.initialize().map_err(|e| OptimisticExecutorError::ExecutionFailed(e.to_string()))?;

    let outcome = executor.execute_block_optimistically(block, &snapshot).await?;

    Ok((
        outcome.transaction_results,
        outcome.conflict_resolution,
    ))
}

/// Helper to convert RwSetTracker conflicts into retry order
pub fn compute_retry_strategy(conflicts: &[ConflictingPair]) -> Vec<usize> {
    // Build conflict graph
    let mut conflict_graph: VecHashSet<usize> = vec![HashSet::new(); conflicts.len()];

    for pair in conflicts {
        conflict_graph[pair.tx_a].insert(pair.tx_b);
        conflict_graph[pair.tx_b].insert(pair.tx_a);
    }

    // Find minimum independent set using greedy algorithm
    let mut ordered = Vec::new();
    let mut excluded = HashSet::new();

    while ordered.len() < conflicts.len() {
        // Find tx with fewest conflicts
        let mut best_idx = usize::MAX;
        let mut min_conflicts = usize::MAX;

        for (idx, conf_set) in conflict_graph.iter().enumerate() {
            if !excluded.contains(&idx) && conf_set.len() < min_conflicts {
                best_idx = idx;
                min_conflicts = conf_set.len();
            }
        }

        ordered.push(best_idx);
        excluded.insert(best_idx);

        // Exclude conflicting txs
        for &conflict in &conflict_graph[best_idx] {
            excluded.insert(conflict);
        }
    }

    ordered
}

/// Data structures for conflict resolution
#[derive(Debug, Clone)]
pub struct ConflictingPair {
    pub tx_a: usize,
    pub tx_b: usize,
}

#[derive(Debug, Clone)]
pub struct TransactionResult {
    pub tx_index: usize,
    pub success: bool,
    pub gas_used: i64,
    pub rw_set: RwSetTracker,
}

// Simplified placeholder - would need full imports
type HashSet<T> = std::collections::HashSet<T>;
type VecHashSet<T> = Vec<std::collections::HashSet<T>>;
