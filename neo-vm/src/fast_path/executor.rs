//! Fast path executor for optimized transaction execution.
//!
//! This module coordinates fast path execution attempts and automatic
//! fallback to full VM when needed.

use super::error::FastPathResult;
use super::classifier::{TransactionClassifier, TransactionComplexity};
use super::nep17::{Nep17Parser, TransferParams};

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
        let fast_path_attempts = self.fast_path_transfers + self.fast_path_blocks + self.fast_path_fallbacks;
        if fast_path_attempts == 0 {
            return 0.0;
        }

        (self.fast_path_fallbacks as f64 / fast_path_attempts as f64) * 100.0
    }
}

/// Fast path executor coordinating optimized execution.
pub struct FastPathExecutor {
    stats: FastPathStats,
}

impl FastPathExecutor {
    /// Creates a new fast path executor.
    pub fn new() -> Self {
        Self {
            stats: FastPathStats::default(),
        }
    }

    /// Gets current execution statistics.
    pub fn stats(&self) -> &FastPathStats {
        &self.stats
    }

    /// Attempts to classify and prepare for fast path execution.
    ///
    /// Returns the complexity classification which can be used to route execution.
    pub fn classify_transaction(&self, script: &[u8]) -> TransactionComplexity {
        TransactionClassifier::classify(script)
    }

    /// Parses NEP-17 transfer parameters for fast path execution.
    ///
    /// This is a preparatory step - actual execution requires storage integration.
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

impl Default for FastPathExecutor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::OpCode;

    #[test]
    fn test_stats_percentage() {
        let stats = FastPathStats {
            fast_path_transfers: 80,
            fast_path_blocks: 10,
            fast_path_fallbacks: 5,
            full_vm_executions: 10,
        };

        // 90 fast path / 100 total = 90%
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

        // 10 fallbacks / 100 attempts = 10%
        assert!((stats.fallback_rate() - 10.0).abs() < 0.01);
    }

    #[test]
    fn test_classify_nep17() {
        let executor = FastPathExecutor::new();

        let mut script = Vec::new();
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(20);
        script.extend_from_slice(&[1u8; 20]);
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(20);
        script.extend_from_slice(&[2u8; 20]);
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(1);
        script.push(100);
        script.push(OpCode::PUSHNULL as u8);
        script.push(OpCode::PUSH4 as u8);
        script.push(OpCode::PACK as u8);
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(8);
        script.extend_from_slice(b"transfer");
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(20);
        script.extend_from_slice(&[3u8; 20]);
        script.push(OpCode::SYSCALL as u8);

        let complexity = executor.classify_transaction(&script);
        assert_eq!(complexity, TransactionComplexity::SimpleTransfer);
    }

    #[test]
    fn test_parse_and_record() {
        let mut executor = FastPathExecutor::new();

        let mut script = Vec::new();
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(20);
        script.extend_from_slice(&[1u8; 20]);
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(20);
        script.extend_from_slice(&[2u8; 20]);
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(1);
        script.push(100);
        script.push(OpCode::PUSHNULL as u8);
        script.push(OpCode::PUSH4 as u8);
        script.push(OpCode::PACK as u8);
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(8);
        script.extend_from_slice(b"transfer");
        script.push(OpCode::PUSHDATA1 as u8);
        script.push(20);
        script.extend_from_slice(&[3u8; 20]);
        script.push(OpCode::SYSCALL as u8);

        let params = executor.parse_nep17_transfer(&script).unwrap();
        assert_eq!(params.from, vec![1u8; 20]);
        assert_eq!(params.to, vec![2u8; 20]);

        executor.record_fast_transfer();
        assert_eq!(executor.stats().fast_path_transfers, 1);
    }
}
