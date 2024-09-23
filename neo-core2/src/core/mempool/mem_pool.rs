use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::sync::atomic::{AtomicBool, Ordering};
use std::cmp::Ordering as CmpOrdering;

use uint256::Uint256;
use crate::core::mempoolevent;
use crate::core::transaction;
use crate::util;

#[derive(Debug, Clone)]
pub struct Error(String);

impl Error {
    pub fn new(msg: &str) -> Error {
        Error(msg.to_string())
    }
}

pub const ERR_INSUFFICIENT_FUNDS: &str = "insufficient funds";
pub const ERR_CONFLICT: &str = "conflicts: insufficient funds for all pooled tx";
pub const ERR_DUP: &str = "already in the memory pool";
pub const ERR_OOM: &str = "out of memory";
pub const ERR_CONFLICTS_ATTRIBUTE: &str = "conflicts with memory pool due to Conflicts attribute";
pub const ERR_ORACLE_RESPONSE: &str = "conflicts with memory pool due to OracleResponse attribute";

#[derive(Clone)]
struct Item {
    txn: Arc<transaction::Transaction>,
    block_stamp: u32,
    data: Option<Box<dyn std::any::Any>>,
}

type Items = Vec<Item>;

#[derive(Default)]
struct UtilityBalanceAndFees {
    balance: Uint256,
    fee_sum: Uint256,
}

pub struct Pool {
    lock: RwLock<()>,
    verified_map: HashMap<util::Uint256, Arc<transaction::Transaction>>,
    verified_txes: Items,
    fees: HashMap<util::Uint160, UtilityBalanceAndFees>,
    conflicts: HashMap<util::Uint256, Vec<util::Uint256>>,
    oracle_resp: HashMap<u64, util::Uint256>,
    capacity: usize,
    fee_per_byte: i64,
    payer_index: usize,
    update_metrics_cb: Option<Box<dyn Fn(usize)>>,
    resend_threshold: u32,
    resend_func: Option<Box<dyn Fn(&transaction::Transaction, &dyn std::any::Any)>>,
    subscriptions_enabled: bool,
    subscriptions_on: AtomicBool,
    stop_ch: std::sync::mpsc::Sender<()>,
    events: std::sync::mpsc::Sender<mempoolevent::Event>,
    sub_ch: std::sync::mpsc::Sender<std::sync::mpsc::Sender<mempoolevent::Event>>,
    unsub_ch: std::sync::mpsc::Sender<std::sync::mpsc::Sender<mempoolevent::Event>>,
}

impl Item {
    fn compare(&self, other: &Item) -> CmpOrdering {
        let self_high = self.txn.has_attribute(transaction::HIGH_PRIORITY);
        let other_high = other.txn.has_attribute(transaction::HIGH_PRIORITY);
        if self_high && !other_high {
            return CmpOrdering::Greater;
        } else if !self_high && other_high {
            return CmpOrdering::Less;
        }

        let fee_cmp = self.txn.fee_per_byte().cmp(&other.txn.fee_per_byte());
        if fee_cmp != CmpOrdering::Equal {
            return fee_cmp;
        }

        self.txn.network_fee().cmp(&other.txn.network_fee())
    }
}

impl Pool {
    pub fn new(
        capacity: usize,
        payer_index: usize,
        enable_subscriptions: bool,
        update_metrics_cb: Option<Box<dyn Fn(usize)>>,
    ) -> Pool {
        let (stop_ch, _) = std::sync::mpsc::channel();
        let (events, _) = std::sync::mpsc::channel();
        let (sub_ch, _) = std::sync::mpsc::channel();
        let (unsub_ch, _) = std::sync::mpsc::channel();

        Pool {
            lock: RwLock::new(()),
            verified_map: HashMap::with_capacity(capacity),
            verified_txes: Vec::with_capacity(capacity),
            fees: HashMap::new(),
            conflicts: HashMap::new(),
            oracle_resp: HashMap::new(),
            capacity,
            fee_per_byte: 0,
            payer_index,
            update_metrics_cb,
            resend_threshold: 0,
            resend_func: None,
            subscriptions_enabled: enable_subscriptions,
            subscriptions_on: AtomicBool::new(false),
            stop_ch,
            events,
            sub_ch,
            unsub_ch,
        }
    }

    pub fn count(&self) -> usize {
        let _lock = self.lock.read().unwrap();
        self.verified_txes.len()
    }

    pub fn contains_key(&self, hash: &util::Uint256) -> bool {
        let _lock = self.lock.read().unwrap();
        self.verified_map.contains_key(hash)
    }

    pub fn has_conflicts(&self, t: &transaction::Transaction, fee: &dyn Feer) -> bool {
        let _lock = self.lock.read().unwrap();

        if self.contains_key(&t.hash()) {
            return true;
        }

        if self.conflicts.contains_key(&t.hash()) {
            return true;
        }

        for attr in t.get_attributes(transaction::CONFLICTS_T) {
            if self.contains_key(&attr.value().as_conflicts().hash) {
                return true;
            }
        }

        false
    }

    pub fn try_add_senders_fee(
        &mut self,
        tx: &transaction::Transaction,
        feer: &dyn Feer,
        need_check: bool,
    ) -> bool {
        let payer = &tx.signers()[self.payer_index].account;
        let mut sender_fee = self.fees.entry(payer.clone()).or_default();

        if need_check {
            match check_balance(tx, &sender_fee) {
                Ok(new_fee_sum) => sender_fee.fee_sum = new_fee_sum,
                Err(_) => return false,
            }
        } else {
            sender_fee.fee_sum += Uint256::from(tx.system_fee() + tx.network_fee());
        }

        self.fees.insert(payer.clone(), sender_fee);
        true
    }

    pub fn add(
        &mut self,
        t: Arc<transaction::Transaction>,
        fee: &dyn Feer,
        data: Option<Box<dyn std::any::Any>>,
    ) -> Result<(), Error> {
        let p_item = Item {
            txn: t.clone(),
            block_stamp: fee.block_height(),
            data,
        };

        let _lock = self.lock.write().unwrap();

        if self.contains_key(&t.hash()) {
            return Err(Error::new(ERR_DUP));
        }

        let conflicts_to_be_removed = self.check_tx_conflicts(&t, fee)?;

        if let Some(attrs) = t.get_attributes(transaction::ORACLE_RESPONSE_T).first() {
            let id = attrs.value().as_oracle_response().id;
            if let Some(h) = self.oracle_resp.get(&id) {
                if self.verified_map[h].network_fee() >= t.network_fee() {
                    return Err(Error::new(ERR_ORACLE_RESPONSE));
                }
                self.remove_internal(h);
            }
            self.oracle_resp.insert(id, t.hash());
        }

        for conflicting_tx in conflicts_to_be_removed {
            self.remove_internal(&conflicting_tx.hash());
        }

        let n = self
            .verified_txes
            .binary_search_by(|item| item.compare(&p_item).reverse())
            .unwrap_or_else(|e| e);

        if self.verified_txes.len() == self.capacity {
            if n == self.verified_txes.len() {
                return Err(Error::new(ERR_OOM));
            }
            let unlucky = self.verified_txes.pop().unwrap();
            self.verified_txes.insert(n, p_item);
            self.remove_from_map_with_fees_and_attrs(&unlucky);
        } else {
            self.verified_txes.insert(n, p_item);
        }

        self.verified_map.insert(t.hash(), t.clone());

        for attr in t.get_attributes(transaction::CONFLICTS_T) {
            let hash = attr.value().as_conflicts().hash;
            self.conflicts.entry(hash).or_default().push(t.hash());
        }

        self.try_add_senders_fee(&t, fee, false);

        if let Some(ref cb) = self.update_metrics_cb {
            cb(self.verified_txes.len());
        }

        if self.subscriptions_on.load(Ordering::SeqCst) {
            self.events.send(mempoolevent::Event {
                event_type: mempoolevent::TransactionAdded,
                tx: t.clone(),
                data: p_item.data.clone(),
            }).unwrap();
        }

        Ok(())
    }

    pub fn remove(&mut self, hash: &util::Uint256) {
        let _lock = self.lock.write().unwrap();
        self.remove_internal(hash);

        if let Some(ref cb) = self.update_metrics_cb {
            cb(self.verified_txes.len());
        }
    }

    fn remove_internal(&mut self, hash: &util::Uint256) {
        if let Some(tx) = self.verified_map.remove(hash) {
            let num = self
                .verified_txes
                .iter()
                .position(|item| item.txn.hash() == *hash)
                .unwrap();

            let itm = self.verified_txes.remove(num);
            self.remove_from_map_with_fees_and_attrs(&itm);
        }
    }

    fn remove_from_map_with_fees_and_attrs(&mut self, itm: &Item) {
        let payer = &itm.txn.signers()[self.payer_index].account;
        let mut sender_fee = self.fees.get_mut(payer).unwrap();
        sender_fee.fee_sum -= Uint256::from(itm.txn.system_fee() + itm.txn.network_fee());

        self.remove_conflicts_of(&itm.txn);

        if let Some(attrs) = itm.txn.get_attributes(transaction::ORACLE_RESPONSE_T).first() {
            self.oracle_resp.remove(&attrs.value().as_oracle_response().id);
        }

        if self.subscriptions_on.load(Ordering::SeqCst) {
            self.events.send(mempoolevent::Event {
                event_type: mempoolevent::TransactionRemoved,
                tx: itm.txn.clone(),
                data: itm.data.clone(),
            }).unwrap();
        }
    }

    fn remove_conflicts_of(&mut self, tx: &transaction::Transaction) {
        for attr in tx.get_attributes(transaction::CONFLICTS_T) {
            let conflicts_hash = attr.value().as_conflicts().hash;
            if let Some(conflicts) = self.conflicts.get_mut(&conflicts_hash) {
                if conflicts.len() == 1 {
                    self.conflicts.remove(&conflicts_hash);
                } else {
                    conflicts.retain(|&h| h != tx.hash());
                }
            }
        }
    }

    pub fn verify(&self, tx: &transaction::Transaction, feer: &dyn Feer) -> bool {
        let _lock = self.lock.read().unwrap();
        self.check_tx_conflicts(tx, feer).is_ok()
    }

    fn check_tx_conflicts(
        &self,
        tx: &transaction::Transaction,
        fee: &dyn Feer,
    ) -> Result<Vec<Arc<transaction::Transaction>>, Error> {
        let payer = &tx.signers()[self.payer_index].account;
        let mut actual_sender_fee = self.fees.get(payer).cloned().unwrap_or_default();

        let mut expected_sender_fee = actual_sender_fee.clone();
        let mut conflicts_to_be_removed = Vec::new();
        let mut conflicting_fee = 0;

        if let Some(conflicting_hashes) = self.conflicts.get(&tx.hash()) {
            for hash in conflicting_hashes {
                let existing_tx = self.verified_map.get(hash).unwrap();
                if existing_tx.has_signer(payer) {
                    conflicting_fee += existing_tx.network_fee();
                }
                conflicts_to_be_removed.push(existing_tx.clone());
            }
        }

        for attr in tx.get_attributes(transaction::CONFLICTS_T) {
            let hash = attr.value().as_conflicts().hash;
            if let Some(existing_tx) = self.verified_map.get(&hash) {
                if !existing_tx.signers().iter().any(|s| s.account == *payer) {
                    return Err(Error::new(ERR_CONFLICTS_ATTRIBUTE));
                }
                conflicting_fee += existing_tx.network_fee();
                conflicts_to_be_removed.push(existing_tx.clone());
            }
        }

        if conflicting_fee != 0 && tx.network_fee() <= conflicting_fee {
            return Err(Error::new(ERR_CONFLICTS_ATTRIBUTE));
        }

        for conflicting_tx in &conflicts_to_be_removed {
            if conflicting_tx.signers()[self.payer_index].account == *payer {
                expected_sender_fee.fee_sum -= Uint256::from(conflicting_tx.system_fee() + conflicting_tx.network_fee());
            }
        }

        check_balance(tx, &expected_sender_fee)?;

        Ok(conflicts_to_be_removed)
    }
}

fn check_balance(
    tx: &transaction::Transaction,
    balance: &UtilityBalanceAndFees,
) -> Result<Uint256, Error> {
    let mut tx_fee = Uint256::from(tx.system_fee() + tx.network_fee());

    if balance.balance < tx_fee {
        return Err(Error::new(ERR_INSUFFICIENT_FUNDS));
    }

    tx_fee += &balance.fee_sum;

    if balance.balance < tx_fee {
        return Err(Error::new(ERR_CONFLICT));
    }

    Ok(tx_fee)
}

pub trait Feer {
    fn block_height(&self) -> u32;
    fn get_utility_token_balance(&self, account: &util::Uint160) -> Uint256;
    fn fee_per_byte(&self) -> i64;
}
