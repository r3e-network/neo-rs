// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved


use std::cmp::Ordering;
use std::collections::{HashMap, BTreeMap};
use std::sync::{Arc, Mutex};

use neo_base::{errors, math::U256};
use crate::{store::{FeeStates, BlockStates}, tx::{Tx, TxAttr}, types::{H160, H256}};


#[derive(Debug, Copy, Clone)]
pub enum TxRemovalReason {
    CapacityExceeded,
    NoLongerValid,
    Conflict,
}


#[derive(Debug, Clone, errors::Error)]
pub enum AddTxError {
    #[error("add-tx: the '{1}' of the tx '{0}' is invalid")]
    InvalidTx(H256, &'static str),

    #[error("add-tx: insufficient funds")]
    InsufficientFunds,

    #[error("add-tx: insufficient funds for all pooled tx")]
    ConflictedTx,

    #[error("add-tx: tx '{0}' already in the tx pool")]
    Duplicated(H256),

    #[error("add-tx: out of the tx pool capacity")]
    OutOfCapacity,

    #[error("add-tx: conflicted with tx pool due to Conflicts attribute")]
    ConflictsAttribute,

    #[error("add-tx: conflicted with tx pool due to OracleResponse attribute")]
    ConflictedOracleResponse,
}


#[derive(Clone)]
pub struct TxPool {
    inner: Arc<Mutex<InnerPool>>,
}

impl TxPool {
    pub fn new(capacity: usize, payer_index: usize) -> Self {
        Self { inner: Arc::new(Mutex::new(InnerPool::new(capacity, payer_index))) }
    }

    pub fn get_verified_txs(&self, limits: usize) -> Vec<Arc<Tx>> {
        self.inner.lock()
            .unwrap()
            .get_verified_txs(limits)
    }

    pub fn remove_stales(&self, is_still_ok: fn(&Tx) -> bool, states: &(impl FeeStates + BlockStates)) -> Vec<PooledTx> {
        self.inner.lock()
            .unwrap()
            .remove_stales(is_still_ok, states)
    }

    pub fn remove_tx(&self, tx: &H256) -> Option<PooledTx> {
        self.inner.lock()
            .unwrap()
            .remove_tx(tx)
    }

    pub fn add_tx(&self, tx: Tx, states: &(impl FeeStates + BlockStates)) -> Result<(), AddTxError> {
        self.inner.lock()
            .unwrap()
            .add_tx(tx, states)
    }
}

#[derive(Debug, Default, Clone)]
pub struct BalanceFee {
    pub balance: U256,
    pub fees: U256,
}

impl BalanceFee {
    #[inline]
    pub fn new(balance: U256) -> Self {
        Self { balance, fees: U256::default() }
    }

    #[inline]
    pub fn check_balance(&self, tx: &Tx) -> Result<U256, AddTxError> {
        let fee = U256::from(tx.fee());
        if self.balance < fee {
            return Err(AddTxError::InsufficientFunds);
        }

        let fee = fee + self.fees;
        if self.balance < fee {
            return Err(AddTxError::ConflictedTx);
        }
        Ok(fee)
    }
}

#[derive(Debug, Clone)]
pub struct PooledTx {
    pub block_stamp: u32,
    pub tx_number: u64,
    pub tx: Arc<Tx>,
}


#[derive(Debug, Eq, PartialEq)]
struct TxScore {
    pub high_priority: bool,
    pub netfee_per_byte: u64,
    pub netfee: u64,
    pub tx_number: u64,
}

impl TxScore {
    #[inline]
    pub fn new(tx_number: u64, tx: &Tx) -> Self {
        let high_priority = tx.attributes.iter()
            .any(|attr| matches!(attr, TxAttr::HighPriority));
        Self {
            high_priority,
            netfee_per_byte: tx.netfee_per_byte(),
            netfee: tx.netfee,
            tx_number,
        }
    }
}


impl PartialOrd for TxScore {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> { Some(self.cmp(other)) }
}

impl Ord for TxScore {
    fn cmp(&self, other: &Self) -> Ordering {
        if self.high_priority && !other.high_priority {
            return Ordering::Greater;
        }

        if !self.high_priority && other.high_priority {
            return Ordering::Less;
        }

        (self.netfee_per_byte, self.netfee, self.tx_number)
            .cmp(&(other.netfee_per_byte, other.netfee, other.tx_number))
    }
}


#[allow(dead_code)]
pub(crate) struct InnerPool {
    payer_index: usize,
    capacity: usize,
    resend_threshold: u32,
    netfee_per_byte: u64,
    tx_number: u64,

    balances: HashMap<H160, BalanceFee>,
    verified: HashMap<H256, PooledTx>,
    sorted: BTreeMap<TxScore, H256>,
    conflicts: HashMap<H256, Vec<H256>>,
    oracles: HashMap<u64, H256>,

}

impl InnerPool {
    pub fn new(capacity: usize, payer_index: usize) -> Self {
        Self {
            capacity,
            payer_index,
            resend_threshold: 0,
            netfee_per_byte: 0,
            tx_number: 0,
            verified: HashMap::new(),
            sorted: BTreeMap::new(),
            conflicts: HashMap::new(),
            balances: HashMap::new(),
            oracles: HashMap::new(),
        }
    }

    #[inline]
    fn next_tx_number(&mut self) -> u64 {
        self.tx_number += 1;
        self.tx_number
    }

    pub fn get_verified_txs(&self, limits: usize) -> Vec<Arc<Tx>> {
        let limits = if limits == 0 { usize::MAX } else { limits };
        self.sorted.iter()
            .rev()
            .filter_map(|(_, x)| self.verified.get(x))
            .take(limits)
            .map(|pooled| pooled.tx.clone())
            .collect()
    }

    pub fn remove_stales(&mut self, is_still_ok: fn(&Tx) -> bool, states: &(impl FeeStates + BlockStates)) -> Vec<PooledTx> {
        let block_index = states.current_block_index();

        let sorted = core::mem::replace(&mut self.sorted, BTreeMap::new());
        let verified = core::mem::replace(&mut self.verified, HashMap::new());
        self.balances = HashMap::new();
        self.conflicts = HashMap::new();
        self.oracles = HashMap::new();

        let netfee_per_byte = states.netfee_per_byte();
        let netfee_changed = netfee_per_byte > self.netfee_per_byte;
        if netfee_per_byte > self.netfee_per_byte {
            self.netfee_per_byte = netfee_per_byte;
        }

        let mut stales = Vec::new();
        let pooled_txs = sorted.iter()
            .filter_map(|(_, x)| verified.get(x).cloned());
        for pooled in pooled_txs {
            if (!netfee_changed || pooled.tx.netfee_per_byte() >= self.netfee_per_byte) &&
                is_still_ok(&pooled.tx) &&
                self.try_add_sender_fee(&pooled.tx, states) { // try_add_sender_fee must be the latest condition

                let block_stamp = pooled.block_stamp;
                let threshold = self.resend_threshold;
                let diff = block_index - block_stamp;

                // resent at threshold, 2*threshold, 4*threshold ...
                if block_index > block_stamp && threshold > 0 && diff % threshold == 0 && (diff / threshold).count_ones() == 1 {
                    stales.push(pooled.clone());
                }

                self.add_tx_inner(pooled);
            } else {
                // TODO: tx removed event
            }
        }

        stales
    }

    #[inline]
    pub fn remove_tx(&mut self, tx: &H256) -> Option<PooledTx> {
        let pooled = self.verified.get(tx).cloned();
        self.remove_inner(tx);

        pooled
    }

    pub fn add_tx(&mut self, mut tx: Tx, states: &(impl FeeStates + BlockStates)) -> Result<(), AddTxError> {
        if tx.hash.is_none() {
            tx.calc_hash_and_size();
        }

        let pooled = PooledTx {
            block_stamp: states.current_block_index(),
            tx_number: self.next_tx_number(),
            tx: Arc::new(tx),
        };
        let tx = &pooled.tx;
        let hash = tx.hash();
        let tx_fee = tx.fee();
        if self.payer_index >= tx.signers.len() {
            return Err(AddTxError::InvalidTx(hash, "signers"));
        }

        if self.verified.contains_key(&hash) {
            return Err(AddTxError::Duplicated(hash));
        }

        let payer = tx.signers[0].account;
        let mut balance = self.balances.entry(payer.clone())
            .or_insert_with(|| BalanceFee::new(states.balance_of(&payer)))
            .clone();

        let conflicts = self.check_conflicts(tx, &balance)?;
        let evicts = self.check_oracle(tx)?;

        conflicts.iter().for_each(|x| self.remove_inner(x));
        evicts.iter().for_each(|x| self.remove_inner(x));

        // check_capacity will be ok if conflicts or evicts not empty.
        let _ = self.check_capacity(pooled.tx_number, tx)?
            .map(|evicted| { self.remove_inner(&evicted); });

        self.add_tx_inner(pooled);

        balance.fees += tx_fee;
        self.balances.insert(payer, balance);

        // TODO: tx add event
        Ok(())
    }

    fn add_tx_inner(&mut self, pooled: PooledTx) {
        let tx = &pooled.tx;
        let hash = tx.hash();
        let score = TxScore::new(pooled.tx_number, tx);
        tx.attributes.iter()
            .filter_map(|attr| if let TxAttr::Conflicts(x) = attr { Some(x) } else { None })
            .for_each(|attr| {
                self.conflicts.entry(attr.hash.clone())
                    .or_insert_with(|| Vec::new())
                    .push(hash)
            });

        tx.attributes.iter()
            .filter_map(|attr| if let TxAttr::OracleResponse(r) = attr { Some(r) } else { None })
            .for_each(|attr| { self.oracles.insert(attr.id, hash.clone()); });

        self.verified.insert(hash, pooled);
        self.sorted.insert(score, hash);
    }

    fn try_add_sender_fee(&mut self, tx: &Tx, states: &impl FeeStates) -> bool {
        let payer = &tx.signers[self.payer_index].account;
        let fee = self.balances.entry(payer.clone())
            .or_insert_with(|| BalanceFee::new(states.balance_of(payer)));

        if fee.check_balance(tx).is_err() {
            return false;
        }

        fee.fees += tx.fee();
        true
    }

    fn remove_inner(&mut self, tx: &H256) {
        let Some(removed) = self.verified.remove(tx) else { return; };
        let score = TxScore::new(removed.tx_number, &removed.tx);
        let _ = self.sorted.remove(&score);

        let payer = &removed.tx.signers[self.payer_index].account;
        if let Some(fee) = self.balances.get_mut(payer) {
            fee.fees -= removed.tx.fee();
        }

        self.remove_conflicts(&removed.tx);

        removed.tx.attributes.iter()
            .filter_map(|attr| if let TxAttr::OracleResponse(r) = attr { Some(r) } else { None })
            .for_each(|oracle| { self.oracles.remove(&oracle.id); });

        // TODO: tx removed event
    }

    fn remove_conflicts(&mut self, tx: &Tx) {
        let hash = tx.hash();
        let conflicted_txs = tx.attributes.iter()
            .filter_map(|attr| if let TxAttr::Conflicts(x) = attr { Some(x) } else { None });
        for conflicted_tx in conflicted_txs {
            let Some(conflicts) = self.conflicts.get_mut(&conflicted_tx.hash) else {
                continue;
            };
            let found = conflicts.iter()
                .enumerate()
                .find(|(_index, x)| hash.eq(x));
            if let Some((index, _)) = found {
                conflicts.remove(index);
            }

            if conflicts.is_empty() {
                self.conflicts.remove(&conflicted_tx.hash);
            }
        }
    }

    fn check_capacity(&self, tx_number: u64, tx: &Tx) -> Result<Option<H256>, AddTxError> {
        if self.sorted.len() >= self.capacity { //
            return Ok(None);
        }

        let score = TxScore::new(tx_number, tx);
        if let Some((min_score, hash)) = self.sorted.first_key_value() {
            if score.cmp(min_score) != Ordering::Greater {
                return Err(AddTxError::OutOfCapacity);
            }
            return Ok(Some(hash.clone()));
        }

        Ok(None)
    }

    fn check_oracle(&self, tx: &Tx) -> Result<Vec<H256>, AddTxError> {
        let mut evicted = Vec::new();
        let mut dedup = Vec::new();
        let oracles = tx.attributes.iter()
            .filter_map(|attr| if let TxAttr::OracleResponse(r) = attr { Some(r) } else { None });
        for oracle in oracles {
            if dedup.iter().find(|&&x| x == oracle.id).is_some() {
                return Err(AddTxError::ConflictedOracleResponse);
            }

            if let Some(oracle_tx) = self.oracles.get(&oracle.id).cloned() {
                if self.verified.get(&oracle_tx).is_some_and(|x| x.tx.netfee >= tx.netfee) {
                    return Err(AddTxError::ConflictedOracleResponse);
                }
                evicted.push(oracle_tx);
            }
            dedup.push(oracle.id);
        }

        Ok(evicted)
    }

    fn check_conflicts(&self, tx: &Tx, sender_fee: &BalanceFee) -> Result<Vec<H256>, AddTxError> {
        let mut fees = 0u64;
        let mut conflicts = Vec::new();

        let hash = tx.hash();
        let payer = &tx.signers[self.payer_index].account;
        if let Some(hashes) = self.conflicts.get(&hash) {
            conflicts = hashes.iter()
                .filter_map(|x| self.verified.get(x))
                .inspect(|x| if x.tx.has_signer(payer) { fees += x.tx.netfee })
                .collect();
        }

        let find_signer = |who: &H160| {
            tx.signers.iter().any(|x| x.account.eq(who))
        };

        let conflicted_txs = tx.attributes.iter()
            .filter_map(|attr| {
                let TxAttr::Conflicts(x) = attr else { return None; };
                self.verified.get(&x.hash)
            });
        for conflicted in conflicted_txs {
            if !conflicted.tx.signers.iter().any(|x| find_signer(&x.account)) {
                return Err(AddTxError::ConflictsAttribute);
            }

            fees += conflicted.tx.netfee;
            conflicts.push(conflicted);
        }

        if fees > 0 && tx.netfee <= fees {
            return Err(AddTxError::ConflictsAttribute);
        }

        let mut balance = sender_fee.clone();
        conflicts.iter()
            .filter(|&x| x.tx.signers[self.payer_index].account.eq(payer))
            .for_each(|x| balance.fees -= x.tx.fee());

        let _ = balance.check_balance(tx)?;
        Ok(conflicts.iter().map(|x| x.tx.hash()).collect())
    }
}
