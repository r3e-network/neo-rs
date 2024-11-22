use std::collections::{HashMap, HashSet, BTreeSet};
use std::hash::Hash;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};
use actix::prelude::*;
use serde::{Serialize, Deserialize};
use getset::{Getters, Setters, MutGetters, CopyGetters};
use crate::ledger::pool_item::PoolItem;
use crate::ledger::transaction_removed_event_args::TransactionRemovedEventArgs;
use crate::ledger::transaction_verification_context::TransactionVerificationContext;
use crate::ledger::verify_result::VerifyResult;
use crate::neo_system::NeoSystem;
use crate::persistence::IStore;
use crate::store::{Snapshot, Store};
use neo_type::H256;
use crate::network::payloads::Transaction;

#[derive(Getters, Setters, MutGetters, CopyGetters, Debug, Clone)]
pub struct MemoryPool {
    #[getset(get = "pub", set = "pub")]
    transaction_added: Option<Box<dyn Fn(&Transaction)>>,
    #[getset(get = "pub", set = "pub")]
    transaction_removed: Option<Box<dyn Fn(&TransactionRemovedEventArgs)>>,

    #[getset(get = "pub", set = "pub")]
    blocks_till_rebroadcast: u32,
    #[getset(get = "pub", set = "pub")]
    rebroadcast_multiplier_threshold: u32,

    #[getset(get = "pub", set = "pub")]
    max_milliseconds_to_reverify_tx: Duration,
    #[getset(get = "pub", set = "pub")]
    max_milliseconds_to_reverify_tx_per_idle: Duration,

    #[getset(get = "pub", set = "pub")]
    system: Arc<NeoSystem>,

    #[getset(get = "pub", set = "pub")]
    tx_rw_lock: RwLock<()>,

    #[getset(get = "pub", set = "pub")]
    unsorted_transactions: HashMap<H256, PoolItem>,
    #[getset(get = "pub", set = "pub")]
    conflicts: HashMap<H256, HashSet<H256>>,
    #[getset(get = "pub", set = "pub")]
    sorted_transactions: BTreeSet<PoolItem>,

    #[getset(get = "pub", set = "pub")]
    unverified_transactions: HashMap<H256, PoolItem>,
    #[getset(get = "pub", set = "pub")]
    unverified_sorted_transactions: BTreeSet<PoolItem>,

    #[getset(get = "pub", set = "pub")]
    capacity: usize,
    #[getset(get = "pub", set = "pub")]
    verification_context: TransactionVerificationContext,
}

impl MemoryPool {
    pub fn new(system: Arc<NeoSystem>) -> Self {
        let capacity = system.settings().memory_pool_max_transactions;
        let max_milliseconds_to_reverify_tx = Duration::from_millis((system.settings().milliseconds_per_block / 3) as u64);
        let max_milliseconds_to_reverify_tx_per_idle = Duration::from_millis((system.settings().milliseconds_per_block / 15) as u64);

        MemoryPool {
            transaction_added: None,
            transaction_removed: None,
            blocks_till_rebroadcast: 10,
            rebroadcast_multiplier_threshold: (capacity / 10) as u32,
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

    pub fn contains_key(&self, hash: &H256) -> bool {
        let _guard = self.tx_rw_lock.read().unwrap();
        self.unsorted_transactions.contains_key(hash) || self.unverified_transactions.contains_key(hash)
    }

    pub fn try_get_value(&self, hash: &H256) -> Option<Transaction> {
        let _guard = self.tx_rw_lock.read().unwrap();
        self.unsorted_transactions.get(hash)
            .or_else(|| self.unverified_transactions.get(hash))
            .map(|item| item.tx().clone())
    }

    pub fn get_verified_transactions(&self) -> Vec<Transaction> {
        let _guard = self.tx_rw_lock.read().unwrap();
        self.unsorted_transactions.values()
            .map(|item| item.tx().clone())
            .collect()
    }

    pub fn get_verified_and_unverified_transactions(&self) -> (Vec<Transaction>, Vec<Transaction>) {
        let _guard = self.tx_rw_lock.read().unwrap();
        let verified = self.sorted_transactions.iter().rev()
            .map(|item| item.tx().clone())
            .collect();
        let unverified = self.unverified_sorted_transactions.iter().rev()
            .map(|item| item.tx().clone())
            .collect();
        (verified, unverified)
    }

    pub fn get_sorted_verified_transactions(&self) -> Vec<Transaction> {
        let _guard = self.tx_rw_lock.read().unwrap();
        self.sorted_transactions.iter().rev()
            .map(|item| item.tx().clone())
            .collect()
    }

    pub fn try_add(&self, tx: Arc<Transaction>, snapshot: &Snapshot) -> VerifyResult {
        let pool_item = Arc::new(PoolItem::new(tx.clone()));

        if self.unsorted_transactions.read().unwrap().contains_key(&tx.hash()) {
            return VerifyResult::AlreadyInPool;
        }

        let mut unsorted_transactions = self.unsorted_transactions.write().unwrap();
        let mut sorted_transactions = self.sorted_transactions.write().unwrap();
        let mut conflicts = self.conflicts.write().unwrap();
        let mut verification_context = self.verification_context.write().unwrap();

        if !self.check_conflicts(&tx, &mut conflicts) {
            return VerifyResult::HasConflicts;
        }

        if !tx.verify_state_dependent(&self.system.settings(), snapshot, &verification_context) {
            return VerifyResult::Invalid;
        }

        unsorted_transactions.insert(tx.hash().clone(), pool_item.clone());
        verification_context.add_transaction(&tx);
        sorted_transactions.insert(pool_item);

        for attr in tx.get_conflict_attributes() {
            conflicts.entry(attr.hash.clone())
                .or_insert_with(HashSet::new)
                .insert(tx.hash().clone());
        }

        if self.count() > self.capacity {
            self.remove_over_capacity();
        }

        VerifyResult::Succeed
    }

    pub fn try_remove_unverified(&mut self, hash: &H256) -> Option<Transaction> {
        let _guard = self.tx_rw_lock.write().unwrap();
        self.unverified_transactions.remove(hash).map(|item| {
            self.unverified_sorted_transactions.remove(&item);
            item.tx().clone()
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

            let verify_result = self.verify_transaction(item.tx(), snapshot);
            if verify_result == VerifyResult::Succeed {
                self.unsorted_transactions.insert(*hash, item.clone());
                self.sorted_transactions.insert(item.clone());
                to_remove.push(*hash);
                verified_count += 1;
            } else if verify_result != VerifyResult::Expired {
                to_remove.push(*hash);
                if let Some(handler) = &self.transaction_removed {
                    handler(&TransactionRemovedEventArgs::new(item.tx().clone(), verify_result));
                }
            }
        }

        for hash in to_remove {
            self.unverified_transactions.remove(&hash);
            self.unverified_sorted_transactions.retain(|item| item.tx().hash() != hash);
        }

        verified_count
    }

    fn verify_transaction(&self, tx: &Transaction, snapshot: &dyn IStore) -> VerifyResult {
        if self.system.native_contracts().ledger().contains_transaction(snapshot, &tx.hash()) {
            return VerifyResult::AlreadyExists;
        }

        let verification_context = TransactionVerificationContext::new(
            self.system.settings().network,
            snapshot,
            self.system.settings().max_transaction_per_block,
        );
        if tx.verify(snapshot, &verification_context).is_err() {
            return VerifyResult::Invalid;
        }

        if self.has_conflicts(tx) {
            return VerifyResult::HasConflicts;
        }

        let fee = tx.get_network_fee(snapshot) + tx.system_fee(snapshot);
        if fee > self.system.settings().max_transaction_fee {
            return VerifyResult::InsufficientFunds;
        }

        if tx.expiration() <= snapshot.height() + 1 {
            return VerifyResult::Expired;
        }

        VerifyResult::Succeed
    }

    fn check_conflicts(&self, tx: &Transaction, conflicts: &mut HashMap<H256, HashSet<H256>>) -> bool {
        let mut conflicts_fee_sum = 0;
        if let Some(conflicting) = conflicts.get(&tx.hash()) {
            for hash in conflicting {
                let unsorted_tx = &self.unsorted_transactions[hash];
                if unsorted_tx.tx().signers().iter().any(|s| s.account() == tx.sender()) {
                    conflicts_fee_sum += unsorted_tx.tx().network_fee();
                }
            }
        }

        for attr in tx.get_conflict_attributes() {
            if let Some(unsorted_tx) = self.unsorted_transactions.get(&attr.hash) {
                if !tx.signers().iter().any(|s| unsorted_tx.tx().signers().iter().any(|us| us.account() == s.account())) {
                    return false;
                }
                conflicts_fee_sum += unsorted_tx.tx().network_fee();
            }
        }

        if conflicts_fee_sum != 0 && conflicts_fee_sum >= tx.network_fee() {
            return false;
        }

        true
    }

    fn remove_over_capacity(&mut self) -> Vec<Transaction> {
        let mut removed_transactions = Vec::new();
        while self.count() > self.capacity {
            if let Some(min_item) = self.get_lowest_fee_transaction() {
                let hash = min_item.tx().hash();
                if self.unsorted_transactions.remove(&hash).is_some() {
                    self.sorted_transactions.remove(&min_item);
                    self.remove_conflicts_of_verified(&min_item);
                    self.verification_context.remove_transaction(min_item.tx());
                } else {
                    self.unverified_transactions.remove(&hash);
                    self.unverified_sorted_transactions.remove(&min_item);
                }
                removed_transactions.push(min_item.tx().clone());
            } else {
                break;
            }
        }
        removed_transactions
    }

    fn get_lowest_fee_transaction(&self) -> Option<PoolItem> {
        self.sorted_transactions.iter().next()
            .map(|item| item.clone())
            .or_else(|| self.unverified_sorted_transactions.iter().next().cloned())
    }

    fn remove_conflicts_of_verified(&mut self, item: &PoolItem) {
        for attr in item.tx().get_conflict_attributes() {
            if let Some(conflicts) = self.conflicts.get_mut(&attr.hash) {
                conflicts.remove(&item.tx().hash());
                if conflicts.is_empty() {
                    self.conflicts.remove(&attr.hash);
                }
            }
        }
    }

    pub fn invalidate_verified_transactions(&mut self) {
        let _guard = self.tx_rw_lock.write().unwrap();
        for item in self.sorted_transactions.iter() {
            self.unverified_transactions.insert(item.tx().hash(), item.clone());
            self.unverified_sorted_transactions.insert(item.clone());
        }
        self.unsorted_transactions.clear();
        self.verification_context = TransactionVerificationContext::new();
        self.sorted_transactions.clear();
        self.conflicts.clear();
    }

    pub fn update_pool_for_block_persisted(&mut self, block: &Block, snapshot: &Snapshot) {
        let mut conflicting_items = Vec::new();
        let _guard = self.tx_rw_lock.write().unwrap();

        let mut conflicts = HashMap::new();
        for tx in &block.transactions {
            self.unsorted_transactions.remove(&tx.hash());
            self.unverified_transactions.remove(&tx.hash());
            let conflicting_signers: Vec<H160> = tx.signers().iter().map(|s| s.account().clone()).collect();
            for attr in tx.get_conflict_attributes() {
                conflicts.entry(attr.hash.clone())
                    .or_insert_with(Vec::new)
                    .extend(conflicting_signers.clone());
            }
        }

        let persisted: HashSet<H256> = block.transactions.iter().map(|t| t.hash()).collect();
        let mut stale = Vec::new();
        for item in &self.sorted_transactions {
            if conflicts.get(&item.tx().hash()).map_or(false, |signers| 
                item.tx().signers().iter().any(|s| signers.contains(&s.account()))) ||
                item.tx().get_conflict_attributes().iter().any(|a| persisted.contains(&a.hash)) {
                stale.push(item.tx().hash());
                conflicting_items.push(item.tx().clone());
            }
        }
        for hash in stale {
            self.unsorted_transactions.remove(&hash);
            self.unverified_transactions.remove(&hash);
        }

        self.invalidate_verified_transactions();

        if !conflicting_items.is_empty() {
            if let Some(handler) = &self.transaction_removed {
                handler(&TransactionRemovedEventArgs {
                    transactions: conflicting_items,
                    reason: TransactionRemovalReason::Conflict,
                });
            }
        }

        if block.index > 0 && !self.system.header_cache().is_empty() {
            return;
        }

        self.reverify_transactions(snapshot, self.max_milliseconds_to_reverify_tx);
    }

    pub fn invalidate_all_transactions(&mut self) {
        let _guard = self.tx_rw_lock.write().unwrap();
        self.invalidate_verified_transactions();
    }

    pub fn reverify_top_unverified_transactions_if_needed(&mut self, max_to_verify: usize, snapshot: &Snapshot) -> bool {
        if !self.system.header_cache().is_empty() {
            return false;
        }

        if !self.unverified_sorted_transactions.is_empty() {
            let verify_count = if self.sorted_transactions.len() > self.system.settings().max_transactions_per_block {
                1
            } else {
                max_to_verify
            };
            self.reverify_transactions(snapshot, Duration::from_millis(self.max_milliseconds_to_reverify_tx_per_idle.as_millis() as u64));
        }

        !self.unverified_transactions.is_empty()
    }

    pub fn clear(&mut self) {
        let _guard = self.tx_rw_lock.write().unwrap();
        self.unsorted_transactions.clear();
        self.conflicts.clear();
        self.sorted_transactions.clear();
        self.unverified_transactions.clear();
        self.unverified_sorted_transactions.clear();
    }
}
// End of Selection
