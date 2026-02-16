use super::*;

impl LedgerContract {
    pub(super) fn try_read_block<S>(&self, snapshot: &S, hash: &UInt256) -> Result<Option<Block>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = block_storage_key(self.id, hash);
        let item = match snapshot.try_get(&key) {
            Some(item) => item,
            None => return Ok(None),
        };

        let trimmed = deserialize_trimmed_block(&item.get_value())?;
        let mut transactions = Vec::with_capacity(trimmed.hashes.len());
        for tx_hash in &trimmed.hashes {
            match self.try_read_transaction_state(snapshot, tx_hash)? {
                Some(state) => {
                    transactions.push(state.transaction().clone());
                }
                _ => {
                    return Ok(None);
                }
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
            let bytes = item.get_value();
            let hash = Self::parse_uint256_storage(&bytes, "block hash bytes")?;
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

        let trimmed = deserialize_trimmed_block(&item.get_value())?;
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

    pub(super) fn read_transaction_record<S>(
        &self,
        snapshot: &S,
        key: &StorageKey,
    ) -> Result<Option<TransactionStateRecord>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        snapshot
            .try_get(key)
            .map(|item| deserialize_transaction_record(&item.get_value()))
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
        let trimmed = TrimmedBlock::from_block(block);
        let block_bytes = serialize_trimmed_block(&trimmed)?;
        put_item(snapshot, block_key, StorageItem::from_bytes(block_bytes));
        tracing::debug!(
            contract_id = self.id,
            block_hash = %block_hash,
            "trimmed block persisted to storage"
        );

        debug_assert_eq!(block.transactions.len(), tx_states.len());

        self.persist_transaction_states(snapshot, tx_states)
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

    pub(crate) fn persist_transaction_states(
        &self,
        snapshot: &DataCache,
        states: &[PersistedTransactionState],
    ) -> Result<()> {
        for state in states {
            self.persist_transaction_state(snapshot, state)?;
        }
        Ok(())
    }

    pub(crate) fn persist_transaction_state(
        &self,
        snapshot: &DataCache,
        state: &PersistedTransactionState,
    ) -> Result<()> {
        let tx_hash = state.transaction_hash();
        let tx_key = transaction_storage_key(self.id, &tx_hash);
        let tx_bytes = serialize_transaction_record(&TransactionStateRecord::Full(state.clone()))?;
        put_item(snapshot, tx_key, StorageItem::from_bytes(tx_bytes));
        Ok(())
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
