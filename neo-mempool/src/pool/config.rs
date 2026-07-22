//! Transaction-pool runtime policy.
//!
//! This module owns operator-controlled memory-pool limits. Consensus and
//! network identity remain in `neo-config`; pool capacity is deliberately not
//! derived from protocol settings.

use std::num::NonZeroUsize;

use thiserror::Error;

/// Default number of transactions retained by the memory pool.
pub const DEFAULT_MAX_TRANSACTIONS: usize = 50_000;

const DEFAULT_MAX_TRANSACTIONS_NON_ZERO: NonZeroUsize =
    match NonZeroUsize::new(DEFAULT_MAX_TRANSACTIONS) {
        Some(value) => value,
        None => panic!("the built-in transaction-pool capacity must be non-zero"),
    };

/// Invalid transaction-pool runtime configuration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Error)]
pub enum TxPoolConfigError {
    /// A bounded pool must retain at least one transaction.
    #[error("transaction-pool capacity must be greater than zero")]
    ZeroCapacity,
}

/// Immutable operator policy for the transaction memory pool.
///
/// This value is owned by `neo-mempool`, not by the chain specification. A
/// node may therefore tune resource use without changing the network identity
/// or consensus rules it follows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TxPoolConfig {
    max_transactions: NonZeroUsize,
}

impl TxPoolConfig {
    /// Creates a bounded transaction-pool configuration.
    ///
    /// # Errors
    ///
    /// Returns [`TxPoolConfigError::ZeroCapacity`] when `max_transactions` is
    /// zero.
    pub const fn new(max_transactions: usize) -> Result<Self, TxPoolConfigError> {
        let Some(max_transactions) = NonZeroUsize::new(max_transactions) else {
            return Err(TxPoolConfigError::ZeroCapacity);
        };
        Ok(Self { max_transactions })
    }

    /// Returns the maximum number of transactions retained by the pool.
    #[must_use]
    pub const fn max_transactions(self) -> usize {
        self.max_transactions.get()
    }
}

impl Default for TxPoolConfig {
    fn default() -> Self {
        Self {
            max_transactions: DEFAULT_MAX_TRANSACTIONS_NON_ZERO,
        }
    }
}

#[cfg(test)]
#[path = "../tests/pool/config.rs"]
mod tests;
