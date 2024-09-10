
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;
use NeoRust::builder::Transaction;
use crate::tx::OracleResponse;
use crate::uint160::UInt160;
use crate::uint256::UInt256;

/// The context used to verify the transaction.
pub struct TransactionVerificationContext {
    /// Store all verified unsorted transactions' senders' fee currently in the memory pool.
    sender_fee: HashMap<UInt160, u64>,

    /// Store oracle responses
    oracle_responses: HashMap<u64, UInt256>,
}

impl TransactionVerificationContext {
    /// Creates a new instance of `TransactionVerificationContext`.
    pub fn new() -> Self {
        Self {
            sender_fee: HashMap::new(),
            oracle_responses: HashMap::new(),
        }
    }

    /// Adds a verified `Transaction` to the context.
    pub fn add_transaction(&mut self, tx: &Transaction) {
        if let Some(oracle) = tx.get_attribute::<OracleResponse>() {
            self.oracle_responses.insert(oracle.id, tx.hash());
        }

        let fee = tx.system_fee() + tx.network_fee();
        *self.sender_fee.entry(tx.sender()).or_insert(0) += fee;
    }

    /// Determine whether the specified `Transaction` conflicts with other transactions.
    pub fn check_transaction(&self, tx: &Transaction, conflicting_txs: &[Transaction], snapshot: &ISnapshot) -> bool {
        let balance = NativeContract::GAS.balance_of(snapshot, &tx.sender());
        let total_sender_fee_from_pool = self.sender_fee.get(&tx.sender()).cloned().unwrap_or(0);

        let mut expected_fee = tx.system_fee() + tx.network_fee() + total_sender_fee_from_pool;
        for conflict_tx in conflicting_txs.iter().filter(|c| c.sender() == tx.sender()) {
            expected_fee -= conflict_tx.network_fee() + conflict_tx.system_fee();
        }

        if balance < expected_fee {
            return false;
        }

        if let Some(oracle) = tx.get_attribute::<OracleResponse>() {
            if self.oracle_responses.contains_key(&oracle.id) {
                return false;
            }
        }

        true
    }

    /// Removes a `Transaction` from the context.
    pub fn remove_transaction(&mut self, tx: &Transaction) {
        let fee = tx.system_fee() + tx.network_fee();
        if let Some(sender_fee) = self.sender_fee.get_mut(&tx.sender()) {
            *sender_fee -= fee;
            if *sender_fee == 0 {
                self.sender_fee.remove(&tx.sender());
            }
        }

        if let Some(oracle) = tx.get_attribute::<OracleResponse>() {
            self.oracle_responses.remove(&oracle.id);
        }
    }
}
