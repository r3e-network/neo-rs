use crate::network::p2p::payloads::{
    Transaction, oracle_response::OracleResponse, transaction_attribute::TransactionAttribute,
};
use crate::persistence::DataCache;
use crate::smart_contract::native::GasToken;
use crate::{UInt160, UInt256};
use num_bigint::BigInt;
use num_traits::Zero;
use std::collections::HashMap;
use std::sync::Arc;

type BalanceProvider = Arc<dyn Fn(&DataCache, &UInt160) -> BigInt + Send + Sync>;

/// Context used by the memory pool to track per-sender fees and oracle responses.
pub struct TransactionVerificationContext {
    sender_fee: HashMap<UInt160, BigInt>,
    oracle_responses: HashMap<u64, UInt256>,
    balance_provider: BalanceProvider,
}

impl TransactionVerificationContext {
    /// Creates a new context using the default balance provider (GAS balance from the snapshot).
    pub fn new() -> Self {
        Self::with_balance_provider(default_balance_provider())
    }

    /// Creates a new context with a custom balance provider. Useful for testing until the
    /// GAS native contract balance queries are fully wired in.
    pub fn with_balance_provider<F>(provider: F) -> Self
    where
        F: Fn(&DataCache, &UInt160) -> BigInt + Send + Sync + 'static,
    {
        let balance_provider: BalanceProvider = Arc::new(provider);
        Self {
            sender_fee: HashMap::new(),
            oracle_responses: HashMap::new(),
            balance_provider,
        }
    }

    /// Returns the total tracked fee for the specified sender.
    pub fn total_fee_for_sender(&self, sender: &UInt160) -> Option<&BigInt> {
        self.sender_fee.get(sender)
    }

    /// Adds a verified transaction to the tracking set.
    pub fn add_transaction(&mut self, tx: &Transaction) {
        let Some(sender) = tx.sender() else {
            return;
        };
        let fee = Self::fee_amount(tx);
        self.sender_fee
            .entry(sender)
            .and_modify(|existing| *existing += &fee)
            .or_insert(fee);

        if let Some(oracle) = Self::oracle_response(tx) {
            self.oracle_responses.insert(oracle.id, tx.hash());
        }
    }

    /// Removes a transaction from the context (after it leaves the pool).
    pub fn remove_transaction(&mut self, tx: &Transaction) {
        let Some(sender) = tx.sender() else {
            return;
        };
        let fee = Self::fee_amount(tx);

        if let Some(existing) = self.sender_fee.get_mut(&sender) {
            *existing -= &fee;
            if existing.is_zero() {
                self.sender_fee.remove(&sender);
            }
        }

        if let Some(oracle) = Self::oracle_response(tx) {
            self.oracle_responses.remove(&oracle.id);
        }
    }

    /// Determine whether the specified transaction passes the mempool checks.
    pub fn check_transaction<'a, I>(
        &self,
        tx: &Transaction,
        conflicting_txs: I,
        snapshot: &DataCache,
    ) -> bool
    where
        I: IntoIterator<Item = &'a Transaction>,
    {
        let Some(sender) = tx.sender() else {
            return true;
        };

        let mut expected_fee = Self::fee_amount(tx);
        if let Some(existing) = self.sender_fee.get(&sender) {
            expected_fee += existing.clone();
        }

        for conflict in conflicting_txs {
            if conflict.sender() == Some(sender) {
                expected_fee -= Self::fee_amount(conflict);
            }
        }

        if self.balance(snapshot, &sender) < expected_fee {
            return false;
        }

        if let Some(oracle) = Self::oracle_response(tx) {
            if self.oracle_responses.contains_key(&oracle.id) {
                return false;
            }
        }

        true
    }

    fn balance(&self, snapshot: &DataCache, account: &UInt160) -> BigInt {
        (self.balance_provider)(snapshot, account)
    }

    fn fee_amount(tx: &Transaction) -> BigInt {
        BigInt::from(tx.system_fee()) + BigInt::from(tx.network_fee())
    }

    fn oracle_response(tx: &Transaction) -> Option<&OracleResponse> {
        tx.attributes().iter().find_map(|attr| match attr {
            TransactionAttribute::OracleResponse(response) => Some(response),
            _ => None,
        })
    }
}

impl Default for TransactionVerificationContext {
    fn default() -> Self {
        Self::new()
    }
}

fn default_balance_provider() -> impl Fn(&DataCache, &UInt160) -> BigInt + Send + Sync + 'static {
    let gas = GasToken::new();
    move |snapshot: &DataCache, account: &UInt160| gas.balance_of_snapshot(snapshot, account)
}
