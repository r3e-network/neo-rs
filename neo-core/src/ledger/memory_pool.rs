use std::collections::{HashMap, HashSet, BTreeSet};
use std::hash::Hash;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use NeoRust::prelude::Transaction;
use crate::ledger::pool_item::PoolItem;
use crate::ledger::transaction_removed_event_args::TransactionRemovedEventArgs;
use crate::ledger::transaction_verification_context::TransactionVerificationContext;
use crate::ledger::verify_result::VerifyResult;
use crate::neo_system::NeoSystem;
use crate::store::Store;
use crate::uint256::UInt256;

pub struct MemoryPool {
    transaction_added: Option<Box<dyn Fn(&Transaction)>>,
    transaction_removed: Option<Box<dyn Fn(&TransactionRemovedEventArgs)>>,

    blocks_till_rebroadcast: u32,
    rebroadcast_multiplier_threshold: u32,

    max_milliseconds_to_reverify_tx: Duration,
    max_milliseconds_to_reverify_tx_per_idle: Duration,

    system: Arc<NeoSystem>,

    tx_rw_lock: RwLock<()>,

    unsorted_transactions: HashMap<UInt256, PoolItem>,
    conflicts: HashMap<UInt256, HashSet<UInt256>>,
    sorted_transactions: BTreeSet<PoolItem>,

    unverified_transactions: HashMap<UInt256, PoolItem>,
    unverified_sorted_transactions: BTreeSet<PoolItem>,

    capacity: usize,
    verification_context: TransactionVerificationContext,
}

impl MemoryPool {
    pub fn new(system: Arc<NeoSystem>) -> Self {
        let capacity = system.settings.memory_pool_max_transactions;
        let max_milliseconds_to_reverify_tx = Duration::from_millis(system.settings.milliseconds_per_block / 3);
        let max_milliseconds_to_reverify_tx_per_idle = Duration::from_millis(system.settings.milliseconds_per_block / 15);

        MemoryPool {
            transaction_added: None,
            transaction_removed: None,
            blocks_till_rebroadcast: 10,
            rebroadcast_multiplier_threshold: capacity / 10,
            max_milliseconds_to_reverify_tx,
            max_milliseconds_to_reverify_tx_per_idle,
            system,
            tx_rw_lock: RwLock::new(()),
            unsorted_transactions: HashMap::new(),
            conflicts: HashMap::new(),
            sorted_transactions: BTreeSet::new(),
            unverified_transactions: HashMap::new(),
            unverified_sorted_transactions: BTreeSet::new(),
            capacity,
            verification_context: TransactionVerificationContext::new(),
        }
    }

    pub fn count(&self) -> usize {
        let _guard = self.tx_rw_lock.read().unwrap();
        self.unsorted_transactions.len() + self.unverified_transactions.len()
    }

    pub fn verified_count(&self) -> usize {
        self.unsorted_transactions.len()
    }

    pub fn unverified_count(&self) -> usize {
        self.unverified_transactions.len()
    }

    pub fn contains_key(&self, hash: &UInt256) -> bool {
        let _guard = self.tx_rw_lock.read().unwrap();
        self.unsorted_transactions.contains_key(hash) || self.unverified_transactions.contains_key(hash)
    }

    pub fn try_get_value(&self, hash: &UInt256) -> Option<Transaction> {
        let _guard = self.tx_rw_lock.read().unwrap();
        self.unsorted_transactions.get(hash)
            .or_else(|| self.unverified_transactions.get(hash))
            .map(|item| item.tx.clone())
    }

    pub fn get_verified_transactions(&self) -> Vec<Transaction> {
        let _guard = self.tx_rw_lock.read().unwrap();
        self.unsorted_transactions.values()
            .map(|item| item.tx.clone())
            .collect()
    }

    pub fn get_verified_and_unverified_transactions(&self) -> (Vec<Transaction>, Vec<Transaction>) {
        let _guard = self.tx_rw_lock.read().unwrap();
        let verified = self.sorted_transactions.iter().rev()
            .map(|item| item.tx.clone())
            .collect();
        let unverified = self.unverified_sorted_transactions.iter().rev()
            .map(|item| item.tx.clone())
            .collect();
        (verified, unverified)
    }

    pub fn get_sorted_verified_transactions(&self) -> Vec<Transaction> {
        let _guard = self.tx_rw_lock.read().unwrap();
        self.sorted_transactions.iter().rev()
            .map(|item| item.tx.clone())
            .collect()
    }
    pub fn try_add(&mut self, tx: &Transaction, snapshot: &Store) -> VerifyResult {
        let _guard = self.tx_rw_lock.write().unwrap();
        
        if self.contains_key(&tx.hash()) {
            return VerifyResult::AlreadyExists;
        }

        if self.count() >= self.capacity {
            return VerifyResult::OutOfMemory;
        }

        let verify_result = self.verify_transaction(tx, snapshot);
        if verify_result != VerifyResult::Succeed {
            return verify_result;
        }

        let pool_item = PoolItem::new(tx.clone(), Instant::now());
        self.unsorted_transactions.insert(tx.hash(), pool_item.clone());
        self.sorted_transactions.insert(pool_item);

        if let Some(handler) = &self.transaction_added {
            handler(tx);
        }

        VerifyResult::Succeed
    }

    pub fn try_remove_unverified(&mut self, hash: &UInt256) -> Option<Transaction> {
        let _guard = self.tx_rw_lock.write().unwrap();
        self.unverified_transactions.remove(hash).map(|item| {
            self.unverified_sorted_transactions.remove(&item);
            item.tx
        })
    }

    pub fn reverify_transactions(&mut self, snapshot: &dyn Store<WriteBatch=()>, max_time: Duration) -> usize {
        let start_time = Instant::now();
        let mut verified_count = 0;

        let _guard = self.tx_rw_lock.write().unwrap();
        let mut to_remove = Vec::new();

        for (hash, item) in &self.unverified_transactions {
            if start_time.elapsed() >= max_time {
                break;
            }

            let verify_result = self.verify_transaction(&item.tx, snapshot);
            if verify_result == VerifyResult::Succeed {
                self.unsorted_transactions.insert(*hash, item.clone());
                self.sorted_transactions.insert(item.clone());
                to_remove.push(*hash);
                verified_count += 1;
            } else if verify_result != VerifyResult::Expired {
                to_remove.push(*hash);
                if let Some(handler) = &self.transaction_removed {
                    handler(&TransactionRemovedEventArgs::new(item.tx.clone(), verify_result));
                }
            }
        }

        for hash in to_remove {
            self.unverified_transactions.remove(&hash);
            self.unverified_sorted_transactions.retain(|item| item.tx.hash() != hash);
        }

        verified_count
    }
    fn verify_transaction(&self, tx: &Transaction, snapshot: &Store) -> VerifyResult {
        // Check if the transaction is already in the blockchain
        if NativeContract::Ledger::contains_transaction(snapshot, &tx.hash()) {
            return VerifyResult::AlreadyExists;
        }

        // Verify transaction validity
        let verification_context = TransactionVerificationContext::new(
            self.system.settings.network,
            snapshot,
            self.system.settings.max_transaction_per_block,
        );
        if let Err(err) = tx.verify(snapshot, &verification_context) {
            return VerifyResult::Invalid;
        }

        // Check for conflicts with other transactions in the memory pool
        if self.has_conflicts(tx) {
            return VerifyResult::HasConflicts;
        }

        // Verify transaction fee
        let fee = tx.get_network_fee(snapshot) + tx.system_fee(snapshot);
        if fee > self.system.settings.max_transaction_fee {
            return VerifyResult::InsufficientFunds;
        }

        // Verify transaction expiration
        if tx.expiration() <= snapshot.height() + 1 {
            return VerifyResult::Expired;
        }

        VerifyResult::Succeed
    }
}
