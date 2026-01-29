//! Fee policy for transaction prioritization

use serde::{Deserialize, Serialize};

/// Fee policy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeePolicy {
    /// Minimum fee per byte (in datoshi)
    pub min_fee_per_byte: i64,

    /// Maximum fee per byte for priority calculation
    pub max_fee_per_byte: i64,

    /// Low priority fee threshold
    pub low_priority_threshold: i64,

    /// High priority fee threshold
    pub high_priority_threshold: i64,

    /// Fee factor for network congestion
    pub congestion_factor: f64,
}

impl Default for FeePolicy {
    fn default() -> Self {
        Self {
            min_fee_per_byte: 1000,              // 0.00001 GAS per byte
            max_fee_per_byte: 10_000_000,        // 0.1 GAS per byte
            low_priority_threshold: 100_000,     // 0.001 GAS
            high_priority_threshold: 10_000_000, // 0.1 GAS
            congestion_factor: 1.0,
        }
    }
}

impl FeePolicy {
    /// Calculate priority score for a transaction
    ///
    /// Higher score = higher priority
    #[must_use] 
    pub fn calculate_priority(&self, network_fee: i64, size: usize) -> i64 {
        if size == 0 {
            return 0;
        }

        let fee_per_byte = network_fee / size as i64;

        // Normalize to 0-1000 range
        let normalized = if fee_per_byte <= self.min_fee_per_byte {
            0
        } else if fee_per_byte >= self.max_fee_per_byte {
            1000
        } else {
            let range = self.max_fee_per_byte - self.min_fee_per_byte;
            (fee_per_byte - self.min_fee_per_byte) * 1000 / range
        };

        // Apply congestion factor
        (normalized as f64 * self.congestion_factor) as i64
    }

    /// Check if a fee is acceptable
    #[must_use] 
    pub fn is_fee_acceptable(&self, network_fee: i64, size: usize) -> bool {
        if size == 0 {
            return false;
        }

        let fee_per_byte = network_fee / size as i64;
        fee_per_byte >= (self.min_fee_per_byte as f64 * self.congestion_factor) as i64
    }

    /// Get minimum required fee for a transaction of given size
    #[must_use] 
    pub fn minimum_fee(&self, size: usize) -> i64 {
        (size as f64 * self.min_fee_per_byte as f64 * self.congestion_factor) as i64
    }

    /// Update congestion factor based on pool utilization
    pub fn update_congestion(&mut self, pool_utilization: f64) {
        // Increase fees as pool fills up
        self.congestion_factor = if pool_utilization < 0.5 {
            1.0
        } else if pool_utilization < 0.75 {
            (pool_utilization - 0.5).mul_add(2.0, 1.0) // 1.0 to 1.5
        } else if pool_utilization < 0.9 {
            (pool_utilization - 0.75).mul_add(6.67, 1.5) // 1.5 to 2.5
        } else {
            (pool_utilization - 0.9).mul_add(25.0, 2.5) // 2.5 to 5.0
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_priority_calculation() {
        let policy = FeePolicy::default();

        // Low fee
        let low_priority = policy.calculate_priority(1_000_000, 100);

        // High fee
        let high_priority = policy.calculate_priority(100_000_000, 100);

        assert!(high_priority > low_priority);
    }

    #[test]
    fn test_fee_acceptability() {
        let policy = FeePolicy::default();

        // Acceptable fee
        assert!(policy.is_fee_acceptable(1_000_000, 100));

        // Too low fee
        assert!(!policy.is_fee_acceptable(1, 100));
    }

    #[test]
    fn test_congestion_update() {
        let mut policy = FeePolicy::default();

        policy.update_congestion(0.3);
        assert_eq!(policy.congestion_factor, 1.0);

        policy.update_congestion(0.8);
        assert!(policy.congestion_factor > 1.5);
    }
}
