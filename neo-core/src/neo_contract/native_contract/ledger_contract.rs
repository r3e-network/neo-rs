
use neo_sdk::{
    prelude::*,
    storage::{StorageContext, StorageMap},
    types::{Block, Header, Transaction, UInt160, UInt256},
    vm::VMState,
};
use std::collections::HashSet;

#[contract]
pub struct LedgerContract {
    storage_map: StorageMap,
}

const PREFIX_BLOCK_HASH: u8 = 9;
const PREFIX_CURRENT_BLOCK: u8 = 12;
const PREFIX_BLOCK: u8 = 5;
const PREFIX_TRANSACTION: u8 = 11;

impl LedgerContract {
    pub fn new() -> Self {
        Self {
            storage_map: StorageMap::new(),
        }
    }

    #[on_persist]
    pub fn on_persist(&mut self, engine: &mut ApplicationEngine) -> Result<(), ContractError> {
        let transactions: Vec<TransactionState> = engine
            .persisting_block()
            .transactions()
            .iter()
            .map(|tx| TransactionState {
                block_index: engine.persisting_block().index(),
                transaction: tx.clone(),
                state: VMState::NONE,
            })
            .collect();

        self.storage_map.put(
            &Self::create_storage_key(PREFIX_BLOCK_HASH, &engine.persisting_block().index().to_be_bytes()),
            &engine.persisting_block().hash().to_vec(),
        )?;

        self.storage_map.put(
            &Self::create_storage_key(PREFIX_BLOCK, &engine.persisting_block().hash().to_vec()),
            &Self::trim(engine.persisting_block()).to_vec(),
        )?;

        for tx in transactions.iter() {
            self.storage_map.put(
                &Self::create_storage_key(PREFIX_TRANSACTION, &tx.transaction.hash().to_vec()),
                &tx.to_vec(),
            )?;

            let conflicting_signers: HashSet<UInt160> = tx.transaction.signers().iter().map(|s| s.account()).collect();
            for attr in tx.transaction.get_attributes::<Conflicts>() {
                self.storage_map.put(
                    &Self::create_storage_key(PREFIX_TRANSACTION, &attr.hash().to_vec()),
                    &TransactionState {
                        block_index: engine.persisting_block().index(),
                        transaction: None,
                        state: VMState::NONE,
                    }
                    .to_vec(),
                )?;

                for signer in conflicting_signers.iter() {
                    self.storage_map.put(
                        &Self::create_storage_key(PREFIX_TRANSACTION, &[&attr.hash().to_vec(), &signer.to_vec()].concat()),
                        &TransactionState {
                            block_index: engine.persisting_block().index(),
                            transaction: None,
                            state: VMState::NONE,
                        }
                        .to_vec(),
                    )?;
                }
            }
        }

        engine.set_state(&transactions);
        Ok(())
    }

    #[post_persist]
    pub fn post_persist(&mut self, engine: &mut ApplicationEngine) -> Result<(), ContractError> {
        let mut state = self
            .storage_map
            .get(&Self::create_storage_key(PREFIX_CURRENT_BLOCK, &[]))
            .map(|data| HashIndexState::from_vec(&data))
            .unwrap_or_default();

        state.hash = engine.persisting_block().hash();
        state.index = engine.persisting_block().index();

        self.storage_map
            .put(&Self::create_storage_key(PREFIX_CURRENT_BLOCK, &[]), &state.to_vec())?;

        Ok(())
    }

    pub fn initialized(&self, snapshot: &StorageContext) -> bool {
        snapshot
            .find(Self::create_storage_key(PREFIX_BLOCK, &[]).as_slice())
            .next()
            .is_some()
    }

    fn is_traceable_block(&self, snapshot: &StorageContext, index: u32, max_traceable_blocks: u32) -> bool {
        let current_index = self.current_index(snapshot);
        if index > current_index {
            return false;
        }
        index + max_traceable_blocks > current_index
    }

    pub fn get_block_hash(&self, snapshot: &StorageContext, index: u32) -> Option<UInt256> {
        self.storage_map
            .get(&Self::create_storage_key(PREFIX_BLOCK_HASH, &index.to_be_bytes()))
            .map(|data| UInt256::from_slice(&data))
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = "read_states")]
    pub fn current_hash(&self, snapshot: &StorageContext) -> UInt256 {
        self.storage_map
            .get(&Self::create_storage_key(PREFIX_CURRENT_BLOCK, &[]))
            .map(|data| HashIndexState::from_vec(&data).hash)
            .unwrap_or_default()
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = "read_states")]
    pub fn current_index(&self, snapshot: &StorageContext) -> u32 {
        self.storage_map
            .get(&Self::create_storage_key(PREFIX_CURRENT_BLOCK, &[]))
            .map(|data| HashIndexState::from_vec(&data).index)
            .unwrap_or_default()
    }

    pub fn contains_block(&self, snapshot: &StorageContext, hash: &UInt256) -> bool {
        self.storage_map
            .contains_key(&Self::create_storage_key(PREFIX_BLOCK, &hash.to_vec()))
    }

    pub fn contains_transaction(&self, snapshot: &StorageContext, hash: &UInt256) -> bool {
        self.get_transaction_state(snapshot, hash).is_some()
    }

    pub fn contains_conflict_hash(
        &self,
        snapshot: &StorageContext,
        hash: &UInt256,
        signers: &[UInt160],
        max_traceable_blocks: u32,
    ) -> bool {
        let stub = match self
            .storage_map
            .get(&Self::create_storage_key(PREFIX_TRANSACTION, &hash.to_vec()))
            .map(|data| TransactionState::from_vec(&data))
        {
            Some(state) if state.transaction.is_none() && self.is_traceable_block(snapshot, state.block_index, max_traceable_blocks) => state,
            _ => return false,
        };

        for signer in signers {
            if let Some(state) = self
                .storage_map
                .get(&Self::create_storage_key(PREFIX_TRANSACTION, &[&hash.to_vec(), &signer.to_vec()].concat()))
                .map(|data| TransactionState::from_vec(&data))
            {
                if self.is_traceable_block(snapshot, state.block_index, max_traceable_blocks) {
                    return true;
                }
            }
        }

        false
    }

    pub fn get_trimmed_block(&self, snapshot: &StorageContext, hash: &UInt256) -> Option<TrimmedBlock> {
        self.storage_map
            .get(&Self::create_storage_key(PREFIX_BLOCK, &hash.to_vec()))
            .map(|data| TrimmedBlock::from_vec(&data))
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = "read_states")]
    pub fn get_block(&self, engine: &ApplicationEngine, index_or_hash: &[u8]) -> Option<TrimmedBlock> {
        let hash = if index_or_hash.len() < UInt256::len() {
            self.get_block_hash(&engine.snapshot_cache, u32::from_be_bytes(index_or_hash.try_into().unwrap()))?
        } else if index_or_hash.len() == UInt256::len() {
            UInt256::from_slice(index_or_hash)
        } else {
            return None;
        };

        let block = self.get_trimmed_block(&engine.snapshot_cache, &hash)?;
        if !self.is_traceable_block(&engine.snapshot_cache, block.index, engine.protocol_settings.max_traceable_blocks) {
            return None;
        }
        Some(block)
    }

    pub fn get_block_full(&self, snapshot: &StorageContext, hash: &UInt256) -> Option<Block> {
        let state = self.get_trimmed_block(snapshot, hash)?;
        Some(Block {
            header: state.header,
            transactions: state
                .hashes
                .iter()
                .filter_map(|h| self.get_transaction(snapshot, h))
                .collect(),
        })
    }

    pub fn get_block_by_index(&self, snapshot: &StorageContext, index: u32) -> Option<Block> {
        let hash = self.get_block_hash(snapshot, index)?;
        self.get_block_full(snapshot, &hash)
    }

    pub fn get_header(&self, snapshot: &StorageContext, hash: &UInt256) -> Option<Header> {
        self.get_trimmed_block(snapshot, hash).map(|b| b.header)
    }

    pub fn get_header_by_index(&self, snapshot: &StorageContext, index: u32) -> Option<Header> {
        let hash = self.get_block_hash(snapshot, index)?;
        self.get_header(snapshot, &hash)
    }

    pub fn get_transaction_state(&self, snapshot: &StorageContext, hash: &UInt256) -> Option<TransactionState> {
        self.storage_map
            .get(&Self::create_storage_key(PREFIX_TRANSACTION, &hash.to_vec()))
            .and_then(|data| {
                let state = TransactionState::from_vec(&data);
                if state.transaction.is_some() {
                    Some(state)
                } else {
                    None
                }
            })
    }

    pub fn get_transaction(&self, snapshot: &StorageContext, hash: &UInt256) -> Option<Transaction> {
        self.get_transaction_state(snapshot, hash).map(|state| state.transaction)
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = "read_states", name = "getTransaction")]
    pub fn get_transaction_for_contract(&self, engine: &ApplicationEngine, hash: UInt256) -> Option<Transaction> {
        let state = self.get_transaction_state(&engine.snapshot_cache, &hash)?;
        if !self.is_traceable_block(&engine.snapshot_cache, state.block_index, engine.protocol_settings.max_traceable_blocks) {
            return None;
        }
        Some(state.transaction)
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = "read_states")]
    pub fn get_transaction_signers(&self, engine: &ApplicationEngine, hash: UInt256) -> Option<Vec<Signer>> {
        let state = self.get_transaction_state(&engine.snapshot_cache, &hash)?;
        if !self.is_traceable_block(&engine.snapshot_cache, state.block_index, engine.protocol_settings.max_traceable_blocks) {
            return None;
        }
        Some(state.transaction.signers().to_vec())
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = "read_states")]
    pub fn get_transaction_vm_state(&self, engine: &ApplicationEngine, hash: UInt256) -> VMState {
        let state = match self.get_transaction_state(&engine.snapshot_cache, &hash) {
            Some(state) if self.is_traceable_block(&engine.snapshot_cache, state.block_index, engine.protocol_settings.max_traceable_blocks) => state,
            _ => return VMState::NONE,
        };
        state.state
    }

    #[contract_method(cpu_fee = 1 << 15, required_flags = "read_states")]
    pub fn get_transaction_height(&self, engine: &ApplicationEngine, hash: UInt256) -> i32 {
        let state = match self.get_transaction_state(&engine.snapshot_cache, &hash) {
            Some(state) if self.is_traceable_block(&engine.snapshot_cache, state.block_index, engine.protocol_settings.max_traceable_blocks) => state,
            _ => return -1,
        };
        state.block_index as i32
    }

    #[contract_method(cpu_fee = 1 << 16, required_flags = "read_states")]
    pub fn get_transaction_from_block(&self, engine: &ApplicationEngine, block_index_or_hash: &[u8], tx_index: i32) -> Option<Transaction> {
        let hash = if block_index_or_hash.len() < UInt256::len() {
            self.get_block_hash(&engine.snapshot_cache, u32::from_be_bytes(block_index_or_hash.try_into().unwrap()))?
        } else if block_index_or_hash.len() == UInt256::len() {
            UInt256::from_slice(block_index_or_hash)
        } else {
            return None;
        };

        let block = self.get_trimmed_block(&engine.snapshot_cache, &hash)?;
        if !self.is_traceable_block(&engine.snapshot_cache, block.index, engine.protocol_settings.max_traceable_blocks) {
            return None;
        }

        if tx_index < 0 || tx_index >= block.hashes.len() as i32 {
            return None;
        }

        self.get_transaction(&engine.snapshot_cache, &block.hashes[tx_index as usize])
    }

    fn create_storage_key(prefix: u8, key: &[u8]) -> Vec<u8> {
        [&[prefix], key].concat()
    }

    fn trim(block: &Block) -> TrimmedBlock {
        TrimmedBlock {
            header: block.header.clone(),
            hashes: block.transactions.iter().map(|tx| tx.hash()).collect(),
        }
    }
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct HashIndexState {
    hash: UInt256,
    index: u32,
}

#[derive(Clone, Serialize, Deserialize)]
struct TransactionState {
    block_index: u32,
    transaction: Transaction,
    state: VMState,
}

#[derive(Clone, Serialize, Deserialize)]
struct TrimmedBlock {
    header: Header,
    hashes: Vec<UInt256>,
}
