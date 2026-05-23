//! Native ledger contract: manages blocks and transactions on-chain.

use self::keys::{
    block_hash_storage_key, block_storage_key, current_block_storage_key,
    transaction_conflict_storage_key, transaction_storage_key,
};
use self::state::{
    deserialize_hash_index_state, deserialize_transaction_record, deserialize_trimmed_block,
    serialize_hash_index_state, serialize_transaction_record, serialize_trimmed_block,
    TransactionStateRecord,
};
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::hardfork::Hardfork;
use crate::ledger::Block;
#[cfg(test)]
use crate::network::p2p::payloads::Transaction;
use crate::persistence::{i_read_only_store::IReadOnlyStoreGeneric, DataCache};
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::{
    policy_contract::PolicyContract, trimmed_block::TrimmedBlock, NativeMethod,
};
use crate::smart_contract::{StorageItem, StorageKey};
use crate::{UInt160, UInt256};
use neo_vm_rs::VmState as VMState;

/// Prefix for block-hash-by-index storage
const PREFIX_BLOCK_HASH: u8 = 9;
/// Prefix for block storage (trimmed block payloads)
const PREFIX_BLOCK: u8 = 5;
/// Prefix for transaction state storage
const PREFIX_TRANSACTION: u8 = 11;
/// Prefix for current block pointer storage
const PREFIX_CURRENT_BLOCK: u8 = 12;

pub(crate) mod keys;
mod metadata;
mod native_impl;
mod state;
pub use state::{LedgerTransactionStates, PersistedTransactionState};

/// LedgerContract native contract
pub struct LedgerContract {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl LedgerContract {
    pub const ID: i32 = -4;

    /// Creates a new LedgerContract instance
    pub fn new() -> Self {
        // LedgerContract hash: 0xda65b600f7124ce6c79950c1772a36403104f2be
        let hash = UInt160::parse("0xda65b600f7124ce6c79950c1772a36403104f2be")
            .expect("Valid LedgerContract hash");

        Self {
            id: Self::ID,
            hash,
            methods: Self::native_methods(),
        }
    }

    /// Gets the current block hash from the persisted state.
    pub fn current_hash<S>(&self, snapshot: &S) -> Result<UInt256>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = current_block_storage_key(self.id);
        if let Some(item) = snapshot.try_get(&key) {
            let state = deserialize_hash_index_state(&item.value_bytes())?;
            return Ok(state.hash);
        }
        Ok(UInt256::default())
    }

    /// Gets the current block index (height) from the persisted state.
    pub fn current_index<S>(&self, snapshot: &S) -> Result<u32>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = current_block_storage_key(self.id);
        if let Some(item) = snapshot.try_get(&key) {
            let state = deserialize_hash_index_state(&item.value_bytes())?;
            return Ok(state.index);
        }
        Ok(0)
    }

    /// Retrieves a block by hash or index.
    pub fn get_block<S>(&self, snapshot: &S, hash_or_index: HashOrIndex) -> Result<Option<Block>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        match hash_or_index {
            HashOrIndex::Hash(hash) => self.try_read_block(snapshot, &hash),
            HashOrIndex::Index(index) => {
                if let Some(hash) = self.load_block_hash(snapshot, index)? {
                    self.try_read_block(snapshot, &hash)
                } else {
                    Ok(None)
                }
            }
        }
    }

    /// Checks whether a block exists in storage.
    pub fn contains_block<S>(&self, snapshot: &S, hash: &UInt256) -> bool
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = block_storage_key(self.id, hash);
        snapshot.try_get(&key).is_some()
    }

    /// Checks whether a transaction exists in storage.
    pub fn contains_transaction<S>(&self, snapshot: &S, hash: &UInt256) -> Result<bool>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        Ok(self.try_read_transaction_state(snapshot, hash)?.is_some())
    }

    pub fn contains_conflict_hash<S>(
        &self,
        snapshot: &S,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> Result<bool>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        if signers.is_empty() {
            return Ok(false);
        }

        let current_index = self.current_index(snapshot)?;
        let key = transaction_storage_key(self.id, hash);

        let Some(TransactionStateRecord::ConflictStub {
            block_index: stub_index,
        }) = self.read_transaction_record(snapshot, &key)?
        else {
            return Ok(false);
        };

        if !Self::is_traceable_block(current_index, stub_index, max_traceable_blocks) {
            return Ok(false);
        }

        for signer in signers {
            let signer_key = transaction_conflict_storage_key(self.id, hash, signer);
            if let Some(TransactionStateRecord::ConflictStub { block_index }) =
                self.read_transaction_record(snapshot, &signer_key)?
            {
                if Self::is_traceable_block(current_index, block_index, max_traceable_blocks) {
                    return Ok(true);
                }
            }
        }

        Ok(false)
    }

    pub fn max_traceable_blocks_snapshot<S>(
        &self,
        snapshot: &S,
        settings: &ProtocolSettings,
    ) -> Result<u32>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let current_index = self.current_index(snapshot)?;
        let mut value = if settings.is_hardfork_enabled(Hardfork::HfEchidna, current_index) {
            max_traceable_blocks_from_snapshot(snapshot, settings.max_traceable_blocks)
        } else {
            settings.max_traceable_blocks
        };

        if value == 0 {
            value = settings.max_traceable_blocks;
        }

        value = value.min(PolicyContract::MAX_MAX_TRACEABLE_BLOCKS);
        Ok(value.max(1))
    }

    fn try_read_block<S>(&self, snapshot: &S, hash: &UInt256) -> Result<Option<Block>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = block_storage_key(self.id, hash);
        let item = match snapshot.try_get(&key) {
            Some(item) => item,
            None => return Ok(None),
        };

        let trimmed = deserialize_trimmed_block(&item.value_bytes())?;
        let mut transactions = Vec::with_capacity(trimmed.hashes.len());
        for tx_hash in &trimmed.hashes {
            if let Some(state) = self.try_read_transaction_state(snapshot, tx_hash)? {
                transactions.push(state.transaction().clone());
            } else {
                return Ok(None);
            }
        }

        Ok(Some(Block::new(trimmed.header, transactions)))
    }

    fn load_block_hash<S>(&self, snapshot: &S, index: u32) -> Result<Option<UInt256>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = block_hash_storage_key(self.id, index);
        if let Some(item) = snapshot.try_get(&key) {
            let bytes = item.value_bytes();
            let hash = UInt256::from_bytes(&bytes)
                .map_err(|e| Error::invalid_data(format!("Invalid block hash bytes: {e}")))?;
            return Ok(Some(hash));
        }
        Ok(None)
    }

    pub fn get_block_hash_by_index<S>(&self, snapshot: &S, index: u32) -> Result<Option<UInt256>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        self.load_block_hash(snapshot, index)
    }

    pub fn get_trimmed_block<S>(&self, snapshot: &S, hash: &UInt256) -> Result<Option<TrimmedBlock>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = block_storage_key(self.id, hash);
        tracing::debug!(
            contract_id = self.id,
            block_hash = %hash,
            storage_key = %hex::encode(key.to_array()),
            "querying trimmed block from storage"
        );
        let Some(item) = snapshot.try_get(&key) else {
            return Ok(None);
        };

        let trimmed = deserialize_trimmed_block(&item.value_bytes())?;
        Ok(Some(trimmed))
    }

    fn try_read_transaction_state<S>(
        &self,
        snapshot: &S,
        hash: &UInt256,
    ) -> Result<Option<PersistedTransactionState>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = transaction_storage_key(self.id, hash);
        match self.read_transaction_record(snapshot, &key)? {
            Some(TransactionStateRecord::Full(state)) => Ok(Some(state)),
            _ => Ok(None),
        }
    }

    fn read_transaction_record<S>(
        &self,
        snapshot: &S,
        key: &StorageKey,
    ) -> Result<Option<TransactionStateRecord>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        snapshot
            .try_get(key)
            .map(|item| deserialize_transaction_record(&item.value_bytes()))
            .transpose()
    }

    pub fn get_transaction_state<S>(
        &self,
        snapshot: &S,
        hash: &UInt256,
    ) -> Result<Option<PersistedTransactionState>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        self.try_read_transaction_state(snapshot, hash)
    }

    fn store_block_state(
        &self,
        snapshot: &DataCache,
        block: &Block,
        tx_states: &[PersistedTransactionState],
    ) -> Result<()> {
        let block_hash = block.hash();
        let index = block.index();
        let trimmed = TrimmedBlock::try_from_block(block)?;
        let block_bytes = serialize_trimmed_block(&trimmed)?;
        let tx_records = self.prepare_transaction_records(tx_states)?;

        // Persist block hash (index -> hash)
        let hash_key = block_hash_storage_key(self.id, index);
        put_item(
            snapshot,
            hash_key,
            StorageItem::from_bytes(block_hash.to_bytes().to_vec()),
        );

        // Persist the block payload
        let block_key = block_storage_key(self.id, &block_hash);
        tracing::debug!(
            contract_id = self.id,
            block_hash = %block_hash,
            storage_key = %hex::encode(block_key.to_array()),
            "persisting trimmed block to storage"
        );
        put_item(snapshot, block_key, StorageItem::from_bytes(block_bytes));
        tracing::debug!(
            contract_id = self.id,
            block_hash = %block_hash,
            "trimmed block persisted to storage"
        );

        debug_assert_eq!(block.transactions.len(), tx_states.len());

        self.put_transaction_records(snapshot, tx_records);
        Ok(())
    }

    fn update_current_block_state(
        &self,
        snapshot: &DataCache,
        hash: &UInt256,
        index: u32,
    ) -> Result<()> {
        let key = current_block_storage_key(self.id);
        let bytes = serialize_hash_index_state(hash, index)?;
        put_item(snapshot, key, StorageItem::from_bytes(bytes));
        Ok(())
    }

    /// Repairs or updates the persisted current block pointer.
    pub fn set_current_block_state(
        &self,
        snapshot: &DataCache,
        hash: &UInt256,
        index: u32,
    ) -> Result<()> {
        self.update_current_block_state(snapshot, hash, index)
    }

    #[allow(dead_code)]
    pub(crate) fn persist_transaction_states(
        &self,
        snapshot: &DataCache,
        states: &[PersistedTransactionState],
    ) -> Result<()> {
        let tx_records = self.prepare_transaction_records(states)?;
        self.put_transaction_records(snapshot, tx_records);
        Ok(())
    }

    #[allow(dead_code)]
    pub(crate) fn persist_transaction_state(
        &self,
        snapshot: &DataCache,
        state: &PersistedTransactionState,
    ) -> Result<()> {
        let tx_hash = state.try_transaction_hash()?;
        let tx_key = transaction_storage_key(self.id, &tx_hash);
        let tx_bytes = serialize_transaction_record(&TransactionStateRecord::Full(state.clone()))?;
        put_item(snapshot, tx_key, StorageItem::from_bytes(tx_bytes));
        Ok(())
    }

    fn prepare_transaction_records(
        &self,
        states: &[PersistedTransactionState],
    ) -> Result<Vec<(UInt256, Vec<u8>)>> {
        states
            .iter()
            .map(|state| {
                let hash = state.try_transaction_hash()?;
                let bytes =
                    serialize_transaction_record(&TransactionStateRecord::Full(state.clone()))?;
                Ok((hash, bytes))
            })
            .collect()
    }

    fn put_transaction_records(&self, snapshot: &DataCache, records: Vec<(UInt256, Vec<u8>)>) {
        for (tx_hash, tx_bytes) in records {
            let tx_key = transaction_storage_key(self.id, &tx_hash);
            put_item(snapshot, tx_key, StorageItem::from_bytes(tx_bytes));
        }
    }

    pub fn update_transaction_vm_state(
        &self,
        snapshot: &DataCache,
        hash: &UInt256,
        vm_state: VMState,
    ) -> Result<()> {
        let key = transaction_storage_key(self.id, hash);
        if let Some(TransactionStateRecord::Full(mut state)) =
            self.read_transaction_record(snapshot, &key)?
        {
            state.set_vm_state(vm_state);
            let updated = serialize_transaction_record(&TransactionStateRecord::Full(state))?;
            put_item(snapshot, key, StorageItem::from_bytes(updated));
        }
        Ok(())
    }

    fn persist_conflict_stub(
        &self,
        snapshot: &DataCache,
        conflict_hash: &UInt256,
        block_index: u32,
        signers: &[UInt160],
    ) -> Result<()> {
        let record = TransactionStateRecord::ConflictStub { block_index };
        let bytes = serialize_transaction_record(&record)?;

        let key = transaction_storage_key(self.id, conflict_hash);
        put_item(snapshot, key, StorageItem::from_bytes(bytes.clone()));

        for signer in signers {
            let signer_key = transaction_conflict_storage_key(self.id, conflict_hash, signer);
            put_item(snapshot, signer_key, StorageItem::from_bytes(bytes.clone()));
        }

        Ok(())
    }

    pub fn update_transaction_vm_states(
        &self,
        snapshot: &DataCache,
        updates: &[(UInt256, VMState)],
    ) -> Result<()> {
        for (hash, vm_state) in updates {
            self.update_transaction_vm_state(snapshot, hash, *vm_state)?;
        }
        Ok(())
    }
}

/// Hash or index parameter for block queries
pub enum HashOrIndex {
    Hash(UInt256),
    Index(u32),
}

fn put_item(snapshot: &DataCache, key: StorageKey, item: StorageItem) {
    if snapshot.get(&key).is_some() {
        snapshot.update(key, item);
    } else {
        snapshot.add(key, item);
    }
}

fn max_traceable_blocks_from_snapshot<S>(snapshot: &S, default: u32) -> u32
where
    S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
{
    PolicyContract::get_max_traceable_blocks_snapshot(snapshot)
        .filter(|&value| value > 0)
        .unwrap_or(default)
}

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;
    use crate::ledger::{Block, BlockHeader};
    use crate::network::p2p::payloads::signer::Signer;
    use crate::network::p2p::payloads::witness::Witness;
    use crate::UInt160;
    use crate::WitnessScope;
    use neo_vm_rs::OpCode;

    fn make_signed_transaction() -> Transaction {
        let mut tx = Transaction::new();
        tx.set_valid_until_block(10);
        tx.add_signer(Signer::new(
            UInt160::default(),
            WitnessScope::CALLED_BY_ENTRY,
        ));
        tx.add_witness(Witness::new());
        tx
    }

    fn make_unserializable_transaction() -> Transaction {
        let mut tx = make_signed_transaction();
        tx.set_script(vec![OpCode::NOP.byte(); u16::MAX as usize + 1]);
        tx
    }

    #[test]
    fn update_vm_state_overwrites_persisted_value() {
        let ledger = LedgerContract::new();
        let snapshot = DataCache::new(false);

        let mut tx = make_signed_transaction();
        tx.set_script(vec![0xAA]);
        let state = PersistedTransactionState::new(&tx, 42);
        ledger
            .persist_transaction_state(&snapshot, &state)
            .expect("persist state");

        let hash = tx.hash();
        ledger
            .update_transaction_vm_state(&snapshot, &hash, VMState::HALT)
            .expect("update state");

        let stored = ledger
            .get_transaction_state(&snapshot, &hash)
            .expect("read state")
            .expect("state present");
        assert_eq!(stored.vm_state(), VMState::HALT);
        assert_eq!(stored.block_index(), 42);
    }

    #[test]
    fn batch_vm_state_update_applies_all_entries() {
        let ledger = LedgerContract::new();
        let snapshot = DataCache::new(false);

        let mut tx1 = make_signed_transaction();
        tx1.set_nonce(100);
        tx1.set_script(vec![0x01]);
        let mut tx2 = make_signed_transaction();
        tx2.set_nonce(200);
        tx2.set_script(vec![0x02]);

        let state1 = PersistedTransactionState::new(&tx1, 1);
        let state2 = PersistedTransactionState::new(&tx2, 2);
        ledger
            .persist_transaction_state(&snapshot, &state1)
            .expect("state1");
        ledger
            .persist_transaction_state(&snapshot, &state2)
            .expect("state2");

        let updates = vec![(tx1.hash(), VMState::FAULT), (tx2.hash(), VMState::HALT)];
        ledger
            .update_transaction_vm_states(&snapshot, &updates)
            .expect("updates");

        let state1 = ledger
            .get_transaction_state(&snapshot, &updates[0].0)
            .unwrap()
            .unwrap();
        let state2 = ledger
            .get_transaction_state(&snapshot, &updates[1].0)
            .unwrap()
            .unwrap();

        assert_eq!(state1.vm_state(), VMState::FAULT);
        assert_eq!(state2.vm_state(), VMState::HALT);
    }

    #[test]
    fn ledger_transaction_states_mark_vm_state() {
        let mut tx = Transaction::new();
        tx.set_script(vec![0x10]);
        let hash = tx.hash();
        let mut states = LedgerTransactionStates::new(vec![PersistedTransactionState::new(&tx, 0)]);
        let updated = states.mark_vm_state(&hash, VMState::FAULT);
        assert!(updated);
        let updates = states.try_into_updates().expect("updates");
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].1, VMState::FAULT);
    }

    #[test]
    fn persist_transaction_state_rejects_unserializable_hash_without_zero_key() {
        let ledger = LedgerContract::new();
        let snapshot = DataCache::new(false);
        let tx = make_unserializable_transaction();
        let state = PersistedTransactionState::new(&tx, 42);

        assert!(ledger.persist_transaction_state(&snapshot, &state).is_err());

        let zero_key = transaction_storage_key(ledger.id, &UInt256::zero());
        assert!(snapshot.get(&zero_key).is_none());
        assert!(snapshot.tracked_items().is_empty());
    }

    #[test]
    fn store_block_state_rejects_unserializable_transaction_before_tracking_writes() {
        let ledger = LedgerContract::new();
        let snapshot = DataCache::new(false);
        let tx = make_unserializable_transaction();
        let block = Block::new(BlockHeader::default(), vec![tx.clone()]);
        let tx_states = vec![PersistedTransactionState::new(&tx, block.index())];

        assert!(ledger
            .store_block_state(&snapshot, &block, &tx_states)
            .is_err());

        let block_hash = block.hash();
        assert!(snapshot
            .get(&block_hash_storage_key(ledger.id, block.index()))
            .is_none());
        assert!(snapshot
            .get(&block_storage_key(ledger.id, &block_hash))
            .is_none());
        assert!(snapshot
            .get(&transaction_storage_key(ledger.id, &UInt256::zero()))
            .is_none());
        assert!(snapshot.tracked_items().is_empty());
    }

    #[test]
    fn ledger_transaction_states_try_into_updates_rejects_unserializable_hash() {
        let tx = make_unserializable_transaction();
        let states = LedgerTransactionStates::new(vec![PersistedTransactionState::new(&tx, 0)]);

        assert!(states.try_into_updates().is_err());
    }
}

impl LedgerContract {
    fn is_traceable_block(current_index: u32, target_index: u32, max_traceable: u32) -> bool {
        if target_index > current_index {
            return false;
        }
        let window_end = target_index.saturating_add(max_traceable);
        window_end > current_index
    }
}
