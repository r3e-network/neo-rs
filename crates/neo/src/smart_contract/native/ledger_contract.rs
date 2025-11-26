//! Native ledger contract: manages blocks and transactions on-chain.

use self::keys::{
    block_hash_storage_key, block_storage_key, current_block_storage_key,
    transaction_conflict_storage_key, transaction_storage_key,
};
use self::state::{
    deserialize_hash_index_state, deserialize_transaction_record, deserialize_trimmed_block,
    serialize_hash_index_state, serialize_transaction, serialize_transaction_record,
    serialize_trimmed_block, TransactionStateRecord,
};
use crate::error::CoreError as Error;
use crate::error::CoreResult as Result;
use crate::hardfork::Hardfork;
use crate::ledger::Block;
use crate::neo_io::BinaryWriter;
use crate::network::p2p::payloads::transaction_attribute::TransactionAttribute;
#[cfg(test)]
use crate::network::p2p::payloads::Transaction;
use crate::persistence::{i_read_only_store::IReadOnlyStoreGeneric, DataCache};
use crate::protocol_settings::ProtocolSettings;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::native::{
    policy_contract::PolicyContract, trimmed_block::TrimmedBlock, NativeContract, NativeMethod,
};
use crate::smart_contract::{StorageItem, StorageKey};
use crate::{UInt160, UInt256};
use neo_vm::vm_state::VMState;

/// Prefix for block-hash-by-index storage
const PREFIX_BLOCK_HASH: u8 = 9;
/// Prefix for block storage (trimmed block payloads)
const PREFIX_BLOCK: u8 = 5;
/// Prefix for transaction state storage
const PREFIX_TRANSACTION: u8 = 11;
/// Prefix for current block pointer storage
const PREFIX_CURRENT_BLOCK: u8 = 12;

pub(crate) mod keys;
mod state;
pub use state::{LedgerTransactionStates, PersistedTransactionState};

/// LedgerContract native contract
pub struct LedgerContract {
    id: i32,
    hash: UInt160,
    methods: Vec<NativeMethod>,
}

impl LedgerContract {
    const ID: i32 = -4;

    /// Creates a new LedgerContract instance
    pub fn new() -> Self {
        // LedgerContract hash: 0xda65b600f7124ce6c79950c1772a36403104f2be
        let hash = UInt160::from_bytes(&[
            0xda, 0x65, 0xb6, 0x00, 0xf7, 0x12, 0x4c, 0xe6, 0xc7, 0x99, 0x50, 0xc1, 0x77, 0x2a,
            0x36, 0x40, 0x31, 0x04, 0xf2, 0xbe,
        ])
        .expect("Valid LedgerContract hash");

        let methods = vec![
            NativeMethod::new("currentHash".to_string(), 1 << 15, true, 0x01),
            NativeMethod::new("currentIndex".to_string(), 1 << 15, true, 0x01),
            NativeMethod::new("getBlock".to_string(), 1 << 15, true, 0x01),
            NativeMethod::new("getTransaction".to_string(), 1 << 15, true, 0x01),
            NativeMethod::new("getTransactionFromBlock".to_string(), 1 << 15, true, 0x01),
            NativeMethod::new("getTransactionHeight".to_string(), 1 << 15, true, 0x01),
            NativeMethod::new("getTransactionSigners".to_string(), 1 << 15, true, 0x01),
            NativeMethod::new("getTransactionVMState".to_string(), 1 << 15, true, 0x01),
            NativeMethod::new("containsBlock".to_string(), 1 << 15, true, 0x01),
            NativeMethod::new("containsTransaction".to_string(), 1 << 15, true, 0x01),
        ];

        Self {
            id: Self::ID,
            hash,
            methods,
        }
    }

    /// Gets the current block hash from the persisted state.
    pub fn current_hash<S>(&self, snapshot: &S) -> Result<UInt256>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let key = current_block_storage_key(self.id);
        if let Some(item) = snapshot.try_get(&key) {
            let state = deserialize_hash_index_state(&item.get_value())?;
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
            let state = deserialize_hash_index_state(&item.get_value())?;
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

        let trimmed = deserialize_trimmed_block(&item.get_value())?;
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
            let bytes = item.get_value();
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

        let trimmed = deserialize_trimmed_block(&item.get_value())?;
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

    fn store_block_state(
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

impl NativeContract for LedgerContract {
    fn id(&self) -> i32 {
        self.id
    }

    fn name(&self) -> &str {
        "LedgerContract"
    }

    fn hash(&self) -> UInt160 {
        self.hash
    }

    fn methods(&self) -> &[NativeMethod] {
        &self.methods
    }

    fn invoke(
        &self,
        engine: &mut ApplicationEngine,
        method: &str,
        args: &[Vec<u8>],
    ) -> Result<Vec<u8>> {
        let snapshot_arc = engine.snapshot_cache();
        let snapshot = snapshot_arc.as_ref();
        let current_index = self.current_index(snapshot)?;
        let max_traceable_blocks = self.resolve_max_traceable_blocks(engine, snapshot);

        match method {
            "currentHash" => {
                if !args.is_empty() {
                    return Err(Error::invalid_argument(
                        "currentHash requires no arguments".to_string(),
                    ));
                }
                let hash = self.current_hash(snapshot)?;
                Ok(hash.to_bytes().to_vec())
            }
            "currentIndex" => {
                if !args.is_empty() {
                    return Err(Error::invalid_argument(
                        "currentIndex requires no arguments".to_string(),
                    ));
                }
                let index = current_index;
                Ok(index.to_le_bytes().to_vec())
            }
            "getBlock" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "getBlock requires 1 argument".to_string(),
                    ));
                }
                let selector = &args[0];
                let target = if selector.len() == 32 {
                    let hash = UInt256::from_bytes(selector)
                        .map_err(|e| Error::invalid_argument(format!("Invalid block hash: {e}")))?;
                    HashOrIndex::Hash(hash)
                } else if selector.len() == 4 {
                    let mut buf = [0u8; 4];
                    buf.copy_from_slice(selector);
                    HashOrIndex::Index(u32::from_le_bytes(buf))
                } else {
                    return Err(Error::invalid_argument(
                        "Invalid selector for getBlock".to_string(),
                    ));
                };

                let maybe_trimmed = match &target {
                    HashOrIndex::Hash(hash) => self.get_trimmed_block(snapshot, hash)?,
                    HashOrIndex::Index(index) => {
                        if let Some(hash) = self.load_block_hash(snapshot, *index)? {
                            self.get_trimmed_block(snapshot, &hash)?
                        } else {
                            None
                        }
                    }
                };

                match maybe_trimmed {
                    Some(trimmed)
                        if Self::is_traceable_block(
                            current_index,
                            trimmed.index(),
                            max_traceable_blocks,
                        ) =>
                    {
                        serialize_trimmed_block(&trimmed)
                    }
                    _ => Ok(Vec::new()),
                }
            }
            "getTransaction" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "getTransaction requires 1 argument".to_string(),
                    ));
                }
                let hash = UInt256::from_bytes(&args[0]).map_err(|e| {
                    Error::invalid_argument(format!("Invalid transaction hash: {e}"))
                })?;
                if let Some(state) = self.get_transaction_state_if_traceable(
                    snapshot,
                    &hash,
                    current_index,
                    max_traceable_blocks,
                )? {
                    let bytes = serialize_transaction(state.transaction())?;
                    Ok(bytes)
                } else {
                    Ok(Vec::new())
                }
            }
            "getTransactionFromBlock" => {
                if args.len() != 2 {
                    return Err(Error::invalid_argument(
                        "getTransactionFromBlock requires 2 arguments".to_string(),
                    ));
                }
                let block_hash = UInt256::from_bytes(&args[0])
                    .map_err(|e| Error::invalid_argument(format!("Invalid block hash: {e}")))?;
                if args[1].len() != 4 {
                    return Err(Error::invalid_argument(
                        "Invalid transaction index".to_string(),
                    ));
                }
                let mut buf = [0u8; 4];
                buf.copy_from_slice(&args[1]);
                let tx_index = u32::from_le_bytes(buf);

                if let Some(tx) = self.get_transaction_from_block(
                    snapshot,
                    &block_hash,
                    tx_index,
                    current_index,
                    max_traceable_blocks,
                )? {
                    let bytes = serialize_transaction(tx.transaction())?;
                    Ok(bytes)
                } else {
                    Ok(Vec::new())
                }
            }
            "getTransactionHeight" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "getTransactionHeight requires 1 argument".to_string(),
                    ));
                }
                let hash = UInt256::from_bytes(&args[0]).map_err(|e| {
                    Error::invalid_argument(format!("Invalid transaction hash: {e}"))
                })?;
                if let Some(state) = self.get_transaction_state_if_traceable(
                    snapshot,
                    &hash,
                    current_index,
                    max_traceable_blocks,
                )? {
                    Ok(state.block_index().to_le_bytes().to_vec())
                } else {
                    Ok(Vec::new())
                }
            }
            "getTransactionSigners" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "getTransactionSigners requires 1 argument".to_string(),
                    ));
                }
                let hash = UInt256::from_bytes(&args[0]).map_err(|e| {
                    Error::invalid_argument(format!("Invalid transaction hash: {e}"))
                })?;
                if let Some(state) = self.get_transaction_state_if_traceable(
                    snapshot,
                    &hash,
                    current_index,
                    max_traceable_blocks,
                )? {
                    let mut writer = BinaryWriter::new();
                    writer
                        .write_serializable_vec(state.transaction().signers())
                        .map_err(|e| Error::serialization(e.to_string()))?;
                    Ok(writer.to_bytes())
                } else {
                    Ok(Vec::new())
                }
            }
            "getTransactionVMState" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "getTransactionVMState requires 1 argument".to_string(),
                    ));
                }
                let hash = UInt256::from_bytes(&args[0]).map_err(|e| {
                    Error::invalid_argument(format!("Invalid transaction hash: {e}"))
                })?;
                if let Some(state) = self.get_transaction_state_if_traceable(
                    snapshot,
                    &hash,
                    current_index,
                    max_traceable_blocks,
                )? {
                    Ok(vec![state.vm_state_raw()])
                } else {
                    Ok(vec![0])
                }
            }
            "containsBlock" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "containsBlock requires 1 argument".to_string(),
                    ));
                }
                let hash = UInt256::from_bytes(&args[0])
                    .map_err(|e| Error::invalid_argument(format!("Invalid block hash: {e}")))?;
                Ok(vec![if self.contains_block(snapshot, &hash) {
                    1
                } else {
                    0
                }])
            }
            "containsTransaction" => {
                if args.len() != 1 {
                    return Err(Error::invalid_argument(
                        "containsTransaction requires 1 argument".to_string(),
                    ));
                }
                let hash = UInt256::from_bytes(&args[0]).map_err(|e| {
                    Error::invalid_argument(format!("Invalid transaction hash: {e}"))
                })?;
                Ok(vec![if self
                    .get_transaction_state_if_traceable(
                        snapshot,
                        &hash,
                        current_index,
                        max_traceable_blocks,
                    )?
                    .is_some()
                {
                    1
                } else {
                    0
                }])
            }
            _ => Err(Error::native_contract(format!(
                "Method {} not found",
                method
            ))),
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn initialize(&self, _engine: &mut ApplicationEngine) -> Result<()> {
        Ok(())
    }

    fn on_persist(&self, engine: &mut ApplicationEngine) -> Result<()> {
        let snapshot = engine.snapshot_cache();
        let block = engine.persisting_block().cloned().ok_or_else(|| {
            Error::native_contract("No current block available for persistence".to_string())
        })?;
        let tx_states: Vec<PersistedTransactionState> = block
            .transactions
            .iter()
            .map(|tx| PersistedTransactionState::new(tx, block.index()))
            .collect();
        engine.set_state(LedgerTransactionStates::new(tx_states.clone()));
        self.store_block_state(snapshot.as_ref(), &block, &tx_states)?;

        for tx in &block.transactions {
            let conflicts: Vec<UInt256> = tx
                .attributes()
                .iter()
                .filter_map(|attr| match attr {
                    TransactionAttribute::Conflicts(conflict) => Some(conflict.hash),
                    _ => None,
                })
                .collect();

            if conflicts.is_empty() {
                continue;
            }

            let signer_accounts: Vec<UInt160> =
                tx.signers().iter().map(|signer| signer.account).collect();

            for conflict_hash in conflicts {
                self.persist_conflict_stub(
                    snapshot.as_ref(),
                    &conflict_hash,
                    block.index(),
                    &signer_accounts,
                )?;
            }
        }

        Ok(())
    }

    fn post_persist(&self, engine: &mut ApplicationEngine) -> Result<()> {
        let snapshot = engine.snapshot_cache();
        let block = engine.persisting_block().ok_or_else(|| {
            Error::native_contract("No current block available for persistence".to_string())
        })?;
        let block_clone = block.clone();
        let hash = block_clone.hash();
        let index = block_clone.index();
        self.update_current_block_state(snapshot.as_ref(), &hash, index)?;

        if let Some(state_cache) = engine.take_state::<LedgerTransactionStates>() {
            let updates = state_cache.into_updates();
            if !updates.is_empty() {
                self.update_transaction_vm_states(snapshot.as_ref(), &updates)?;
            }
        }

        Ok(())
    }
}

impl Default for LedgerContract {
    fn default() -> Self {
        Self::new()
    }
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::network::p2p::payloads::signer::Signer;
    use crate::network::p2p::payloads::witness::Witness;
    use crate::UInt160;
    use crate::WitnessScope;

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
        let updates = states.into_updates();
        assert_eq!(updates.len(), 1);
        assert_eq!(updates[0].1, VMState::FAULT);
    }
}

impl LedgerContract {
    fn resolve_max_traceable_blocks<S>(&self, engine: &ApplicationEngine, snapshot: &S) -> u32
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        let settings = engine.protocol_settings();
        let mut value = if engine.is_hardfork_enabled(Hardfork::HfEchidna) {
            max_traceable_blocks_from_snapshot(snapshot, settings.max_traceable_blocks)
        } else {
            settings.max_traceable_blocks
        };

        if value == 0 {
            value = settings.max_traceable_blocks;
        }

        value = value.min(PolicyContract::MAX_MAX_TRACEABLE_BLOCKS);
        value.max(1)
    }

    fn is_traceable_block(current_index: u32, target_index: u32, max_traceable: u32) -> bool {
        if target_index > current_index {
            return false;
        }
        let window_end = target_index.saturating_add(max_traceable);
        window_end > current_index
    }

    fn get_transaction_state_if_traceable<S>(
        &self,
        snapshot: &S,
        hash: &UInt256,
        current_index: u32,
        max_traceable: u32,
    ) -> Result<Option<PersistedTransactionState>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        if let Some(state) = self.try_read_transaction_state(snapshot, hash)? {
            if Self::is_traceable_block(current_index, state.block_index(), max_traceable) {
                return Ok(Some(state));
            }
        }
        Ok(None)
    }

    fn get_transaction_from_block<S>(
        &self,
        snapshot: &S,
        block_hash: &UInt256,
        tx_index: u32,
        current_index: u32,
        max_traceable: u32,
    ) -> Result<Option<PersistedTransactionState>>
    where
        S: IReadOnlyStoreGeneric<StorageKey, StorageItem>,
    {
        if let Some(block) = self.try_read_block(snapshot, block_hash)? {
            if !Self::is_traceable_block(current_index, block.index(), max_traceable) {
                return Ok(None);
            }

            if let Some(tx) = block.transactions.get(tx_index as usize) {
                return self.get_transaction_state_if_traceable(
                    snapshot,
                    &tx.hash(),
                    current_index,
                    max_traceable,
                );
            }
        }
        Ok(None)
    }
}
