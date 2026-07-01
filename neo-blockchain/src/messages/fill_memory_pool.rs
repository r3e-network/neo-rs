//! Request to fill the memory pool with transactions.

use neo_payloads::Transaction;
use serde::{Deserialize, Serialize};

/// Request to fill the memory pool with transactions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FillMemoryPool {
    /// Transactions to add to the memory pool.
    pub transactions: Vec<Transaction>,
}
