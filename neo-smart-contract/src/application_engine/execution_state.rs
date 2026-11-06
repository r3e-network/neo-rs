//! Typed transaction execution state shared with native persistence.
use crate::error::CoreResult as Result;
use neo_primitives::UInt256;
use neo_protocol::transaction::Transaction;
use neo_vm::VmState as VMState;
use serde::{Deserialize, Serialize};

/// A transaction persisted on-chain together with its containing block index and
/// VM execution state (C# `TransactionState`).
#[derive(Clone, Serialize, Deserialize)]
pub struct PersistedTransactionState {
    block_index: u32,
    vm_state: u8,
    transaction: Transaction,
}

impl PersistedTransactionState {
    /// Creates a persisted state for `tx` recorded in the block at `block_index`.
    pub fn new(tx: &Transaction, block_index: u32) -> Self {
        Self {
            block_index,
            vm_state: VMState::NONE.to_byte(),
            transaction: tx.clone(),
        }
    }

    /// Returns the index of the block containing the transaction.
    pub fn block_index(&self) -> u32 {
        self.block_index
    }

    /// Returns the raw VM state byte stored on-chain.
    pub fn vm_state_raw(&self) -> u8 {
        self.vm_state
    }

    /// Returns the VM execution state (`HALT`/`FAULT`) of the transaction.
    pub fn vm_state(&self) -> VMState {
        VMState::from_byte(self.vm_state)
    }

    /// Updates the persisted VM execution state of the transaction.
    pub fn set_vm_state(&mut self, vm_state: VMState) {
        self.vm_state = vm_state.to_byte();
    }

    /// Returns a reference to the stored transaction.
    pub fn transaction(&self) -> &Transaction {
        &self.transaction
    }

    /// Returns a mutable reference to the stored transaction.
    pub fn transaction_mut(&mut self) -> &mut Transaction {
        &mut self.transaction
    }

    /// Returns the transaction hash, panicking if it cannot be computed.
    pub fn transaction_hash(&self) -> UInt256 {
        self.try_transaction_hash()
            .expect("persisted transaction hash must be serializable")
    }

    /// Returns the transaction hash, or an error if it cannot be computed.
    pub fn try_transaction_hash(&self) -> Result<UInt256> {
        self.transaction
            .try_hash()
            .map_err(crate::error::CoreError::from)
    }
}

/// An ordered collection of persisted transaction states for a block.
#[derive(Clone, Default)]
pub struct LedgerTransactionStates {
    states: Vec<PersistedTransactionState>,
    /// Lazily built hash -> index map. The per-transaction execution loop calls
    /// `mark_vm_state` once for every transaction; without this map a block
    /// with N transactions paid O(N^2) hash lookups.
    index: Option<std::collections::HashMap<UInt256, usize>>,
}

impl LedgerTransactionStates {
    /// Creates a collection from the given transaction states.
    pub fn new(states: Vec<PersistedTransactionState>) -> Self {
        Self {
            states,
            index: None,
        }
    }

    /// Returns the contained transaction states in order.
    pub fn states(&self) -> &[PersistedTransactionState] {
        &self.states
    }

    fn ensure_index(&mut self) {
        if self.index.is_some() {
            return;
        }
        let mut index = std::collections::HashMap::with_capacity(self.states.len());
        for (position, state) in self.states.iter().enumerate() {
            match state.try_transaction_hash() {
                Ok(hash) => {
                    index.entry(hash).or_insert(position);
                }
                Err(error) => {
                    tracing::warn!(
                        target: "neo",
                        block_index = state.block_index(),
                        %error,
                        "skipping transaction VM-state index because transaction hash failed"
                    );
                }
            }
        }
        self.index = Some(index);
    }

    /// Sets the VM state of the transaction with `hash`, returning whether a match was found.
    pub fn mark_vm_state(&mut self, hash: &UInt256, vm_state: VMState) -> bool {
        self.ensure_index();
        let Some(position) = self
            .index
            .as_ref()
            .and_then(|index| index.get(hash).copied())
        else {
            return false;
        };
        self.states[position].set_vm_state(vm_state);
        true
    }

    /// Consumes the collection into `(hash, VM state)` pairs, panicking if any hash cannot be computed.
    pub fn into_updates(self) -> Vec<(UInt256, VMState)> {
        self.try_into_updates()
            .expect("persisted transaction hashes must be serializable")
    }

    /// Consumes the collection into `(hash, VM state)` pairs, failing if any hash cannot be computed.
    pub fn try_into_updates(self) -> Result<Vec<(UInt256, VMState)>> {
        self.states
            .into_iter()
            .map(|state| {
                let hash = state.try_transaction_hash()?;
                Ok((hash, state.vm_state()))
            })
            .collect()
    }
}
