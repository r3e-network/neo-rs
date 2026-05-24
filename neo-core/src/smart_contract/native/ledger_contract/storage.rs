use super::keys::{
    block_hash_storage_key, block_storage_key, current_block_storage_key,
    transaction_conflict_storage_key, transaction_storage_key,
};
use super::state::{
    deserialize_hash_index_state, deserialize_transaction_record, deserialize_trimmed_block,
    serialize_hash_index_state, serialize_transaction_record, serialize_trimmed_block,
    TransactionStateRecord,
};
use super::{HashOrIndex, LedgerContract, PersistedTransactionState};
use crate::error::{CoreError as Error, CoreResult as Result};
use crate::hardfork::Hardfork;
use crate::ledger::Block;
use crate::persistence::{read_only_store::IReadOnlyStoreGeneric, DataCache};
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::native::{policy_contract::PolicyContract, trimmed_block::TrimmedBlock};
use crate::smart_contract::{StorageItem, StorageKey};
use crate::{UInt160, UInt256};
use neo_vm_rs::VmState as VMState;

impl LedgerContract {
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

    pub(super) fn try_read_block<S>(&self, snapshot: &S, hash: &UInt256) -> Result<Option<Block>>
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

    pub(super) fn load_block_hash<S>(&self, snapshot: &S, index: u32) -> Result<Option<UInt256>>
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

    pub(super) fn try_read_transaction_state<S>(
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

    pub(super) fn store_block_state(
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

        let hash_key = block_hash_storage_key(self.id, index);
        put_item(
            snapshot,
            hash_key,
            StorageItem::from_bytes(block_hash.to_bytes().to_vec()),
        );

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

    pub(super) fn update_current_block_state(
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

    pub(super) fn persist_conflict_stub(
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

fn put_item(snapshot: &DataCache, key: StorageKey, item: StorageItem) {
    if snapshot.get(&key).is_some() {
        snapshot.update(key, item);
    } else {
        snapshot.add(key, item);
    }
}

pub(super) fn max_traceable_blocks_from_snapshot<S>(snapshot: &S, default: u32) -> u32
where
    S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
{
    PolicyContract::get_max_traceable_blocks_snapshot(snapshot)
        .filter(|&value| value > 0)
        .unwrap_or(default)
}
