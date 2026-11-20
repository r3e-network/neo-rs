//! Fast path classification and statistics for transaction scripts.
//!
//! This module recognizes transaction templates (complexity classification,
//! NEP-17 transfer parsing) and keeps the counters used for metrics. It never
//! executes a transfer itself; actual fast-path execution lives in
//! `direct_nep17.rs` and [`super::super::execution_strategy`].

use neo_vm::fast_path::FastPathResult;

use super::classifier::{TransactionClassifier, TransactionComplexity};
use super::nep17_parser::{Nep17Parser, TransferParams};

/// Fast path execution statistics.
#[derive(Debug, Clone, Default)]
pub struct FastPathStats {
    /// Number of NEP-17 transfers executed via fast path
    pub fast_path_transfers: u64,

    /// Number of block scripts executed via fast path
    pub fast_path_blocks: u64,

    /// Number of fast path attempts that fell back to full VM
    pub fast_path_fallbacks: u64,

    /// Number of transactions executed via full VM
    pub full_vm_executions: u64,
}

impl FastPathStats {
    /// Calculates the percentage of transactions using fast path.
    pub fn fast_path_percentage(&self) -> f64 {
        let total = self.fast_path_transfers + self.fast_path_blocks + self.full_vm_executions;
        if total == 0 {
            return 0.0;
        }

        let fast_path_total = self.fast_path_transfers + self.fast_path_blocks;
        (fast_path_total as f64 / total as f64) * 100.0
    }

    /// Calculates the fallback rate (percentage of fast path attempts that failed).
    pub fn fallback_rate(&self) -> f64 {
        let fast_path_attempts =
            self.fast_path_transfers + self.fast_path_blocks + self.fast_path_fallbacks;
        if fast_path_attempts == 0 {
            return 0.0;
        }

        (self.fast_path_fallbacks as f64 / fast_path_attempts as f64) * 100.0
    }
}

/// Classifies transaction scripts for metrics and diagnostics.
///
/// Plain classification/statistics infrastructure: it runs the template
/// recognisers and tracks hit/fallback counters, but it performs no direct
/// execution — see `direct_nep17.rs` and [`super::super::execution_strategy`]
/// for actual fast-path execution.
pub struct FastPathClassifier {
    stats: FastPathStats,
}

impl FastPathClassifier {
    /// Creates a new fast path classifier.
    pub fn new() -> Self {
        Self {
            stats: FastPathStats::default(),
        }
    }

    /// Gets current execution statistics.
    pub fn stats(&self) -> &FastPathStats {
        &self.stats
    }

    /// Classifies a script and reports its execution complexity.
    ///
    /// The result is informational only — routing and execution decisions are
    /// made by [`super::super::execution_strategy`].
    pub fn classify_transaction(&self, script: &[u8]) -> TransactionComplexity {
        TransactionClassifier::classify(script)
    }

    /// Parses NEP-17 transfer parameters for inspection and metrics.
    ///
    /// This is a preparatory step only - actual execution requires storage
    /// integration and happens in `direct_nep17.rs`.
    pub fn parse_nep17_transfer(&mut self, script: &[u8]) -> FastPathResult<TransferParams> {
        match Nep17Parser::parse_transfer_script(script) {
            Ok(params) => {
                // Successfully parsed - ready for fast path
                Ok(params)
            }
            Err(e) => {
                // Parse failed - will need to fall back to full VM
                self.stats.fast_path_fallbacks += 1;
                Err(e)
            }
        }
    }

    /// Records a successful fast path transfer execution.
    pub fn record_fast_transfer(&mut self) {
        self.stats.fast_path_transfers += 1;
    }

    /// Records a successful fast path block script execution.
    pub fn record_fast_block_script(&mut self) {
        self.stats.fast_path_blocks += 1;
    }

    /// Records a fallback to full VM.
    pub fn record_fallback(&mut self) {
        self.stats.fast_path_fallbacks += 1;
    }

    /// Records a full VM execution.
    pub fn record_full_vm(&mut self) {
        self.stats.full_vm_executions += 1;
    }

    /// Resets statistics counters.
    pub fn reset_stats(&mut self) {
        self.stats = FastPathStats::default();
    }
}

impl Default for FastPathClassifier {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::super::nep17_parser::standard_transfer_script;
    use super::*;

    #[test]
    fn test_stats_percentage() {
        let stats = FastPathStats {
            fast_path_transfers: 80,
            fast_path_blocks: 10,
            fast_path_fallbacks: 5,
            full_vm_executions: 10,
        };
        assert!((stats.fast_path_percentage() - 90.0).abs() < 0.01);
    }

    #[test]
    fn test_stats_fallback_rate() {
        let stats = FastPathStats {
            fast_path_transfers: 80,
            fast_path_blocks: 10,
            fast_path_fallbacks: 10,
            full_vm_executions: 10,
        };
        assert!((stats.fallback_rate() - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_classify_parse_and_record() {
        let mut executor = FastPathClassifier::new();
        let script = standard_transfer_script(100);
        assert_eq!(
            executor.classify_transaction(&script),
            TransactionComplexity::SimpleTransfer
        );
        let params = executor.parse_nep17_transfer(&script).unwrap();
        assert_eq!(params.from, vec![1u8; 20]);
        assert_eq!(params.to, vec![2u8; 20]);
        executor.record_fast_transfer();
        assert_eq!(executor.stats().fast_path_transfers, 1);
    }
}
