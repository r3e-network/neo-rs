use std::collections::{HashMap, LinkedList};
use std::sync::{RwLock, Arc};
use std::error::Error;

use crate::core::transaction::Witness;
use crate::crypto::hash::{Hashable, Uint256};
use crate::network::payload::{Extensible};
use crate::util::Uint160;

pub trait Ledger: Send + Sync {
    fn block_height(&self) -> u32;
    fn is_extensible_allowed(&self, sender: Uint160) -> bool;
    fn verify_witness(&self, sender: Uint160, hashable: &dyn Hashable, witness: &Witness, gas: i64) -> Result<i64, Box<dyn Error>>;
}

pub struct Pool {
    lock: RwLock<()>,
    verified: HashMap<Uint256, LinkedList<Extensible>>,
    senders: HashMap<Uint160, LinkedList<Extensible>>,
    single_cap: usize,
    chain: Arc<dyn Ledger>,
}

impl Pool {
    pub fn new(chain: Arc<dyn Ledger>, capacity: usize) -> Self {
        if capacity == 0 {
            panic!("invalid capacity");
        }

        Pool {
            lock: RwLock::new(()),
            verified: HashMap::new(),
            senders: HashMap::new(),
            single_cap: capacity,
            chain,
        }
    }

    pub fn add(&self, e: Extensible) -> Result<bool, Box<dyn Error>> {
        if let Err(err) = self.verify(&e) {
            return Err(err);
        }

        let _lock = self.lock.write().unwrap();

        let h = e.hash();
        if self.verified.contains_key(&h) {
            return Ok(false);
        }

        let lst = self.senders.entry(e.sender).or_insert_with(LinkedList::new);
        if lst.len() >= self.single_cap {
            if let Some(value) = lst.pop_front() {
                self.verified.remove(&value.hash());
            }
        }

        lst.push_back(e.clone());
        self.verified.insert(h, lst.clone());
        Ok(true)
    }

    fn verify(&self, e: &Extensible) -> Result<(), Box<dyn Error>> {
        self.chain.verify_witness(e.sender, e, &e.witness, EXTENSIBLE_VERIFY_MAX_GAS)?;

        let h = self.chain.block_height();
        if h < e.valid_block_start || e.valid_block_end <= h {
            if e.valid_block_end == h {
                return Ok(());
            }
            return Err(Box::new(InvalidHeightError));
        }

        if !self.chain.is_extensible_allowed(e.sender) {
            return Err(Box::new(DisallowedSenderError));
        }

        Ok(())
    }

    pub fn get(&self, h: Uint256) -> Option<Extensible> {
        let _lock = self.lock.read().unwrap();
        self.verified.get(&h).and_then(|lst| lst.front().cloned())
    }

    pub fn remove_stale(&self, index: u32) {
        let _lock = self.lock.write().unwrap();

        self.senders.retain(|&s, lst| {
            lst.retain(|e| {
                let h = e.hash();
                if e.valid_block_end <= index || !self.chain.is_extensible_allowed(e.sender) {
                    self.verified.remove(&h);
                    false
                } else if self.chain.verify_witness(e.sender, e, &e.witness, EXTENSIBLE_VERIFY_MAX_GAS).is_err() {
                    self.verified.remove(&h);
                    false
                } else {
                    true
                }
            });
            !lst.is_empty()
        });
    }
}

const EXTENSIBLE_VERIFY_MAX_GAS: i64 = 6000000;

#[derive(Debug)]
struct InvalidHeightError;

impl std::fmt::Display for InvalidHeightError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "invalid height")
    }
}

impl Error for InvalidHeightError {}

#[derive(Debug)]
struct DisallowedSenderError;

impl std::fmt::Display for DisallowedSenderError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "disallowed sender")
    }
}

impl Error for DisallowedSenderError {}
