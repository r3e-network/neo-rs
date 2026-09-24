#![allow(missing_docs, dead_code)] // WIP detector; docs pending cleanup
//! Conflict Detection Engine - Wave 1 Parallel Execution
//!
//! This module detects and resolves conflicts between parallel transaction executions
//! using optimistic concurrency control. It enables multiple transactions to execute
//! speculatively, then identifies conflicts that require re-execution.
//!
//! # Design Philosophy
//!
//! The conflict detector follows these principles:
//! 1. **Optimistic Execution**: Execute all transactions concurrently without locking
//! 2. **Conflict Detection**: Use RwSet comparison to identify conflicts
//! 3. **Retry Strategy**: Re-execute conflicting transactions sequentially
//! 4. **Determinism**: Always resolve in consistent order for protocol compatibility

use crate::state_service::prefetch_pipeline::ExecutedTransaction;
use crate::{Transaction, UInt256};
use neo_vm::RwSetTracker;
use std::collections::HashSet;
use std::sync::Arc;

/// Result of conflict detection and resolution algorithm
#[derive(Debug, Clone)]
pub struct ConflictResolution {
    /// Transaction indices that can commit immediately (no conflicts)
    pub non_conflicting_order: Vec<usize>,

    /// Transactions that need re-execution due to conflicts
    pub needs_retry: Vec<RetryCandidate>,
}

/// Candidate for re-execution due to detected conflicts
#[derive(Debug, Clone)]
pub struct RetryCandidate {
    /// Original position in block
    pub original_index: usize,

    /// Reason for retry (for logging/debugging)
    pub reason: String,

    /// Which committed state height to restore before re-execution
    pub required_state_height: u64,

    /// All tx indices this conflicts with
    pub conflicted_with: Vec<usize>,
}

/// Executed transaction result with its read/write set for conflict detection
#[derive(Debug, Clone)]
pub struct ExecutedTransactionWithRwSet {
    /// Original execution result
    pub executed_tx: ExecutedTransaction,

    /// Read/write set tracked during execution
    pub rw_set: RwSetTracker,
}

/// Detects and resolves conflicts between parallel transaction executions
///
/// Uses an efficient O(n²) algorithm (optimized with hash sets) to detect conflicts:
/// - Write-write conflicts: Both tx modify same key
/// - Write-read conflicts: One writes after other reads
///
/// # Thread Safety
///
/// ConflictDetector is NOT thread-safe. Each worker thread should have its own
/// instance if parallel conflict detection is needed (though typically done post-execution).
pub struct ConflictDetector {
    /// Ordered list of tx results with their rw_sets
    current_block_executions: Vec<ExecutedTransactionWithRwSet>,

    /// Pre-computed hashes for fast lookup
    write_key_hashes: Vec<Vec<u64>>,
    read_key_hashes: Vec<Vec<u64>>,
}

impl ConflictDetector {
    /// Creates a new empty conflict detector
    #[must_use]
    pub fn new() -> Self {
        Self {
            current_block_executions: Vec::new(),
            write_key_hashes: Vec::new(),
            read_key_hashes: Vec::new(),
        }
    }

    /// Submits a transaction result for conflict checking
    ///
    /// Must be called after execution completes but before calling `resolve_conflicts`.
    /// Call this once per transaction in the block.
    ///
    /// # Arguments
    ///
    /// * `tx_index` - Original position of transaction in block
    /// * `executed_tx` - Full execution result including RW set
    pub fn submit_tx(&mut self, tx_index: usize, result: ExecutedTransactionWithRwSet) {
        // Ensure vector is large enough (should be sequential)
        while self.current_block_executions.len() <= tx_index {
            self.current_block_executions
                .push(ExecutedTransactionWithRwSet {
                    executed_tx: ExecutedTransaction {
                        height: 0,
                        hash: UInt256::zero(),
                        tx_index: 0,
                        transaction: Arc::new(Transaction::default()),
                        execution_result: Err(crate::CoreError::system("missing")),
                    },
                    rw_set: RwSetTracker::new(),
                });
        }

        // Store the result
        self.current_block_executions[tx_index] = result;

        // Pre-compute hash values for faster conflict detection
        let _ = self._compute_hash_signature_for_tx(tx_index);
    }

    /// Helper to compute hash signatures for a single tx
    fn _compute_hash_signature_for_tx(&self, tx_idx: usize) -> Vec<(u64, bool)> {
        // Returns (hash, is_write_set) tuples
        let mut hashes = Vec::with_capacity(
            self.current_block_executions[tx_idx].rw_set.read_keys.len()
                + self.current_block_executions[tx_idx]
                    .rw_set
                    .write_keys
                    .len(),
        );

        for key in &self.current_block_executions[tx_idx].rw_set.read_keys {
            hashes.push((Self::_hash_uint256(key), false));
        }

        for key in &self.current_block_executions[tx_idx].rw_set.write_keys {
            hashes.push((Self::_hash_uint256(key), true));
        }

        hashes
    }

    fn _compute_hash_signatures(&mut self) {
        // Recompute all hash signatures for current executions
        self.write_key_hashes.clear();
        self.read_key_hashes.clear();

        for exec in &self.current_block_executions {
            let mut write_set: HashSet<u64> = HashSet::new();
            let mut read_set: HashSet<u64> = HashSet::new();

            for key in &exec.rw_set.write_keys {
                write_set.insert(Self::_hash_uint256(key));
            }

            for key in &exec.rw_set.read_keys {
                read_set.insert(Self::_hash_uint256(key));
            }

            self.write_key_hashes
                .push(write_set.iter().copied().collect());
            self.read_key_hashes
                .push(read_set.iter().copied().collect());
        }
    }

    fn _hash_uint256(value: &UInt256) -> u64 {
        // Simple hash: take first 8 bytes
        let bytes = value.as_ref();
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&bytes[..8]);
        u64::from_le_bytes(buf)
    }

    /// Resolves conflicts and returns ordering/retry strategy
    ///
    /// This is the main entry point after all transactions have been submitted.
    /// Returns two lists:
    /// 1. Non-conflicting transactions that can commit immediately
    /// 2. Conflicting transactions that need sequential re-execution
    ///
    /// # Algorithm
    ///
    /// Uses topological sort approach:
    /// 1. Build conflict graph from rw_sets
    /// 2. Identify independent transactions (no edges)
    /// 3. Mark dependent transactions for retry
    /// 4. Return ordered execution plan
    ///
    /// # Performance Notes
    ///
    /// O(n² × m) where n = number of txs, m = average keys per tx
    /// Optimized with hash sets for O(1) lookups instead of linear search
    pub fn resolve_conflicts(mut self) -> Result<ConflictResolution, ConflictResolutionError> {
        let num_txs = self.current_block_executions.len();

        if num_txs == 0 {
            return Ok(ConflictResolution {
                non_conflicting_order: Vec::new(),
                needs_retry: Vec::new(),
            });
        }

        // Recompute hash signatures
        self._compute_hash_signatures();

        let mut non_conflicting = Vec::new();
        let mut needs_retry = Vec::new();
        let mut committed_indices: HashSet<usize> = HashSet::new();

        // Process transactions in order
        // First pass: find fully independent transactions
        for (idx, exec) in self.current_block_executions.iter().enumerate() {
            if !exec.rw_set.has_writes() {
                // Read-only txs never cause conflicts, but their reads must
                // still be checked against later writers per RwSetTracker's
                // documented write-read conflict semantics.
                non_conflicting.push(idx);
                committed_indices.insert(idx);
                continue;
            }

            // Check if this tx conflicts with any already-committed tx
            let mut has_conflict = false;
            for &committed_idx in &committed_indices {
                let other = &self.current_block_executions[committed_idx];

                if exec.rw_set.conflicts_with(&other.rw_set) {
                    has_conflict = true;
                    break;
                }
            }

            if has_conflict {
                // Wait to see if it can join non-conflicting set later
            } else {
                // Safe to commit now
                non_conflicting.push(idx);
                committed_indices.insert(idx);
            }
        }

        // Second pass: mark remaining as needing retry
        for (idx, exec) in self.current_block_executions.iter().enumerate() {
            if !committed_indices.contains(&idx) && exec.rw_set.has_writes() {
                // Find who we conflict with
                let mut conflicts_with = Vec::new();
                for &committed_idx in &committed_indices {
                    let other = &self.current_block_executions[committed_idx];
                    if exec.rw_set.conflicts_with(&other.rw_set) {
                        conflicts_with.push(committed_idx);
                    }
                }

                needs_retry.push(RetryCandidate {
                    original_index: idx,
                    reason: format!("conflicts with tx(s): {:?}", conflicts_with),
                    required_state_height: 0, // Will be computed by executor
                    conflicted_with: conflicts_with,
                });
            }
        }

        // Sort non-conflicting by original index for deterministic ordering
        non_conflicting.sort();

        Ok(ConflictResolution {
            non_conflicting_order: non_conflicting,
            needs_retry,
        })
    }

    /// Computes the final merged read/write set for the entire block
    ///
    /// Merges all rw_sets into a single unified view representing total
    /// state changes made by the block. Used for:
    /// - State root computation
    /// - Merkle tree construction
    /// - Verification against C# reference implementation
    ///
    /// # Merge Strategy
    ///
    /// 1. Union all read sets (all keys accessed by any tx)
    /// 2. Final write set comes from last writer wins policy
    ///    (matches C# behavior)
    ///
    /// # Returns
    ///
    /// Combined RwSetTracker containing block-level access patterns
    #[must_use]
    pub fn compute_final_rw_set(&self) -> RwSetTracker {
        let mut merged = RwSetTracker::new();
        merged.reserve_read_capacity(
            self.current_block_executions
                .iter()
                .map(|e| e.rw_set.total_accesses())
                .sum::<usize>(),
        );

        for exec in &self.current_block_executions {
            // Aggregate all read keys
            for key in &exec.rw_set.read_keys {
                merged.record_read(*key);
            }

            // Track all writes (final value determines outcome)
            for key in &exec.rw_set.write_keys {
                merged.record_write(*key);
            }

            // Combine notify/log events
            merged.notify_events.extend(&exec.rw_set.notify_events);
            merged.log_events.extend(&exec.rw_set.log_events);
        }

        // Sum up gas consumption
        for exec in &self.current_block_executions {
            merged.set_gas_used(merged.gas_used + exec.rw_set.gas_used);
        }

        merged
    }

    /// Returns number of submitted transactions
    #[must_use]
    pub fn tx_count(&self) -> usize {
        self.current_block_executions.len()
    }

    /// Clears all submitted transactions for reuse
    pub fn clear(&mut self) {
        self.current_block_executions.clear();
        self.write_key_hashes.clear();
        self.read_key_hashes.clear();
    }

    /// Returns reference to specific transaction's execution result
    #[must_use]
    pub fn get_transaction(&self, index: usize) -> Option<&ExecutedTransactionWithRwSet> {
        self.current_block_executions.get(index)
    }
}

impl Default for ConflictDetector {
    fn default() -> Self {
        Self::new()
    }
}

/// Error types for conflict resolution failures
#[derive(thiserror::Error, Debug, Clone, PartialEq, Eq)]
pub enum ConflictResolutionError {
    /// Attempted to resolve before all transactions submitted
    #[error("Attempted to resolve conflicts before all transactions submitted")]
    IncompleteSubmission,

    /// Invalid transaction index
    #[error("Invalid transaction index: {0}")]
    InvalidIndex(usize),

    /// Duplicate transaction submission
    #[error("Duplicate submission for transaction index: {0}")]
    DuplicateSubmission(usize),

    /// Generic error with description
    #[error("Conflict resolution error: {0}")]
    Generic(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Transaction;
    use crate::state_service::prefetch_pipeline::ExecutedTransaction;

    fn make_key(value: u64) -> UInt256 {
        let mut bytes = [0; 32];
        bytes[..8].copy_from_slice(&value.to_le_bytes());
        UInt256::from(bytes)
    }

    fn make_mock_exec(
        tx_index: usize,
        _write_keys: &[u64],
        _read_keys: &[u64],
    ) -> ExecutedTransaction {
        let tx = Transaction::default();
        ExecutedTransaction {
            height: 1000,
            hash: UInt256::zero(),
            tx_index,
            transaction: Arc::new(tx),
            execution_result: Ok(vec![1, 2, 3]),
        }
    }

    #[test]
    fn test_new_detector_is_empty() {
        let detector = ConflictDetector::new();
        assert_eq!(detector.tx_count(), 0);
    }

    #[test]
    fn test_single_tx_no_conflicts() {
        let mut detector = ConflictDetector::new();

        let mut rw_set = RwSetTracker::new();
        rw_set.record_write(make_key(42u64));

        let exec = make_mock_exec(0, &[42], &[]);
        detector.submit_tx(
            0,
            ExecutedTransactionWithRwSet {
                executed_tx: exec,
                rw_set,
            },
        );

        let resolution = detector.resolve_conflicts().unwrap();
        assert_eq!(resolution.non_conflicting_order.len(), 1);
        assert!(resolution.needs_retry.is_empty());
    }

    #[test]
    fn test_two_tx_same_write_key_conflict() {
        let mut detector = ConflictDetector::new();

        // Tx 0 writes key 100
        let mut rw_set0 = RwSetTracker::new();
        rw_set0.record_write(make_key(100u64));

        // Tx 1 also writes key 100 → CONFLICT
        let mut rw_set1 = RwSetTracker::new();
        rw_set1.record_write(make_key(100u64));

        detector.submit_tx(
            0,
            ExecutedTransactionWithRwSet {
                executed_tx: make_mock_exec(0, &[100], &[]),
                rw_set: rw_set0,
            },
        );

        detector.submit_tx(
            1,
            ExecutedTransactionWithRwSet {
                executed_tx: make_mock_exec(1, &[100], &[]),
                rw_set: rw_set1,
            },
        );

        let resolution = detector.resolve_conflicts().unwrap();
        assert_eq!(resolution.non_conflicting_order.len(), 1);
        assert_eq!(resolution.needs_retry.len(), 1);
        assert!(
            resolution.needs_retry[0].original_index == 1
                || resolution.needs_retry[0].original_index == 0
        );
    }

    #[test]
    fn test_read_write_conflict() {
        let mut detector = ConflictDetector::new();

        // Tx 0 reads key 200
        let mut rw_set0 = RwSetTracker::new();
        rw_set0.record_read(make_key(200u64));

        // Tx 1 writes key 200 → CONFLICTS with tx 0's read
        let mut rw_set1 = RwSetTracker::new();
        rw_set1.record_write(make_key(200u64));

        detector.submit_tx(
            0,
            ExecutedTransactionWithRwSet {
                executed_tx: make_mock_exec(0, &[200], &[200]),
                rw_set: rw_set0,
            },
        );

        detector.submit_tx(
            1,
            ExecutedTransactionWithRwSet {
                executed_tx: make_mock_exec(1, &[200], &[]),
                rw_set: rw_set1,
            },
        );

        let resolution = detector.resolve_conflicts().unwrap();
        assert_eq!(resolution.needs_retry.len(), 1);
    }

    #[test]
    fn test_no_conflict_different_keys() {
        let mut detector = ConflictDetector::new();

        // Tx 0 uses key 111
        let mut rw_set0 = RwSetTracker::new();
        rw_set0.record_write(make_key(111u64));

        // Tx 1 uses different key 222
        let mut rw_set1 = RwSetTracker::new();
        rw_set1.record_write(make_key(222u64));

        detector.submit_tx(
            0,
            ExecutedTransactionWithRwSet {
                executed_tx: make_mock_exec(0, &[111], &[]),
                rw_set: rw_set0,
            },
        );

        detector.submit_tx(
            1,
            ExecutedTransactionWithRwSet {
                executed_tx: make_mock_exec(1, &[222], &[]),
                rw_set: rw_set1,
            },
        );

        let resolution = detector.resolve_conflicts().unwrap();
        assert_eq!(resolution.non_conflicting_order.len(), 2);
        assert!(resolution.needs_retry.is_empty());
    }

    #[test]
    fn test_compute_final_rw_set() {
        let mut detector = ConflictDetector::new();

        let mut rw_set0 = RwSetTracker::new();
        rw_set0.record_read(make_key(1u64));
        rw_set0.record_write(make_key(2u64));
        rw_set0.set_gas_used(1000);

        let mut rw_set1 = RwSetTracker::new();
        rw_set1.record_read(make_key(3u64));
        rw_set1.record_write(make_key(4u64));
        rw_set1.set_gas_used(2000);

        detector.submit_tx(
            0,
            ExecutedTransactionWithRwSet {
                executed_tx: make_mock_exec(0, &[2], &[1]),
                rw_set: rw_set0,
            },
        );

        detector.submit_tx(
            1,
            ExecutedTransactionWithRwSet {
                executed_tx: make_mock_exec(1, &[4], &[3]),
                rw_set: rw_set1,
            },
        );

        let final_rw_set = detector.compute_final_rw_set();

        assert_eq!(final_rw_set.read_keys.len(), 2);
        assert_eq!(final_rw_set.write_keys.len(), 2);
        assert_eq!(final_rw_set.gas_used, 3000);
    }

    #[test]
    fn test_empty_detector_resolves() {
        let detector = ConflictDetector::new();
        let resolution = detector.resolve_conflicts().unwrap();

        assert!(resolution.non_conflicting_order.is_empty());
        assert!(resolution.needs_retry.is_empty());
    }
}
