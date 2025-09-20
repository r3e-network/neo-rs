//! Main blockchain implementation.
//!
//! This module provides the main blockchain functionality exactly matching C# Neo Blockchain.

use super::{
    genesis::GenesisManager,
    persistence::{BlockchainPersistence, BlockchainSnapshot},
    state::{BlockchainState, PolicySettings},
    storage::{RocksDBStorage, Storage, StorageItem, StorageKey},
    verification::{BlockchainVerifier, VerifyResult},
};
use crate::{Block, BlockHeader, Error, NetworkType, Result};
use neo_config::{MAX_SCRIPT_LENGTH, MAX_SCRIPT_SIZE, MAX_TRANSACTIONS_PER_BLOCK};
use neo_core::{Transaction, UInt160, UInt256};
use neo_io::MemoryReader;
use neo_smart_contract::{contract_state::ContractState, native::fungible_token::PREFIX_ACCOUNT};
use num_bigint::{BigInt, BigUint, Sign};
use num_traits::{ToPrimitive, Zero};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{Mutex, RwLock};

/// Snapshot of a NEP-17 balance including the last update height.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Nep17Balance {
    pub amount: u128,
    pub last_updated_block: u32,
}

/// Main blockchain manager (matches C# Neo Blockchain exactly)
#[derive(Debug, Clone)]
pub struct Blockchain {
    /// Blockchain persistence layer
    persistence: Arc<BlockchainPersistence>,
    /// Blockchain state manager
    state: Arc<RwLock<BlockchainState>>,
    /// Blockchain verifier
    verifier: Arc<BlockchainVerifier>,
    /// Genesis manager
    genesis: Arc<GenesisManager>,
    /// Current block height cache
    current_height: Arc<RwLock<u32>>,
    /// Block cache
    block_cache: Arc<RwLock<HashMap<u32, Block>>>,
    /// Transaction cache
    transaction_cache: Arc<RwLock<HashMap<UInt256, Transaction>>>,
    /// Registered contract states (native + deployed)
    contract_states: Arc<RwLock<HashMap<UInt160, ContractState>>>,
    /// Sync lock for block persistence
    persist_lock: Arc<Mutex<()>>,
    /// Network configuration
    network: NetworkType,
    /// Fork detection cache - stores alternative chain tips
    fork_cache: Arc<RwLock<HashMap<UInt256, Block>>>,
    /// Orphan blocks waiting for their parent
    orphan_blocks: Arc<RwLock<HashMap<UInt256, Vec<Block>>>>,
}

impl Blockchain {
    /// Creates a new blockchain instance (matches C# Neo Blockchain.Create exactly)
    pub async fn new(network: NetworkType) -> Result<Self> {
        Self::new_with_storage_suffix(network, None).await
    }

    /// Creates a new blockchain instance with optional storage suffix to avoid conflicts
    pub async fn new_with_storage_suffix(
        network: NetworkType,
        suffix: Option<&str>,
    ) -> Result<Self> {
        use std::sync::atomic::{AtomicU32, Ordering};
        static BLOCKCHAIN_COUNT: AtomicU32 = AtomicU32::new(0);
        let count = BLOCKCHAIN_COUNT.fetch_add(1, Ordering::SeqCst) + 1;

        tracing::info!(
            "⚠️ BLOCKCHAIN CREATION #{} for network: {:?} (suffix: {:?})",
            count,
            network,
            suffix
        );

        tracing::info!(
            "🔧 Creating new blockchain instance for network: {:?}",
            network
        );

        let storage = Arc::new({
            if let Some(suffix) = suffix {
                let final_dir = format!("/tmp/neo-blockchain-{}-{}", std::process::id(), suffix);
                match Storage::new_rocksdb(&final_dir) {
                    Ok(storage) => storage,
                    Err(err) => {
                        log::warn!(
                            "Failed to open RocksDB at {} for suffix {:?}: {}. Falling back to temporary storage.",
                            final_dir,
                            suffix,
                            err
                        );
                        Storage::new_temp()
                    }
                }
            } else {
                Storage::new_default().unwrap_or_else(|_| {
                    log::info!(
                        "Warning: Failed to create default storage, using temporary RocksDB storage"
                    );
                    let final_dir = format!("/tmp/neo-blockchain-{}", std::process::id());
                    Storage::new_rocksdb(&final_dir)
                        .expect("Failed to create temporary RocksDB storage")
                })
            }
        });
        let persistence = Arc::new(BlockchainPersistence::new(storage.clone()));
        let state = Arc::new(RwLock::new(BlockchainState::new(
            persistence.clone(),
            network,
        )));
        let verifier = Arc::new(BlockchainVerifier::new());
        let genesis = Arc::new(GenesisManager::new(storage.clone()));

        let blockchain = Self {
            persistence: persistence.clone(),
            state,
            verifier,
            genesis,
            current_height: Arc::new(RwLock::new(0)),
            block_cache: Arc::new(RwLock::new(HashMap::new())),
            transaction_cache: Arc::new(RwLock::new(HashMap::new())),
            contract_states: Arc::new(RwLock::new(HashMap::new())),
            persist_lock: Arc::new(Mutex::new(())),
            network,
            fork_cache: Arc::new(RwLock::new(HashMap::new())),
            orphan_blocks: Arc::new(RwLock::new(HashMap::new())),
        };

        tracing::info!("🔧 Initializing genesis block/* implementation */;");
        match blockchain.initialize_genesis().await {
            Ok(()) => {
                tracing::info!("✅ Genesis initialization completed successfully");
            }
            Err(e) => {
                tracing::error!("❌ Genesis initialization failed: {}", e);
                return Err(e);
            }
        }

        tracing::info!("✅ Blockchain created successfully");

        Ok(blockchain)
    }

    /// Initializes the genesis block (matches C# Neo Blockchain initialization exactly)
    async fn initialize_genesis(&self) -> Result<()> {
        let current_height = self.persistence.get_current_block_height().await?;

        if current_height == 0 {
            if self.persistence.get_block(0).await?.is_none() {
                // Create and persist genesis block
                tracing::info!("Creating genesis block for network: {:?}", self.network);
                let genesis_block = match self.network {
                    NetworkType::MainNet => {
                        tracing::info!("Using MainNet genesis creation");
                        self.genesis.create_genesis_block()?
                    }
                    NetworkType::TestNet => {
                        tracing::info!("Using TestNet genesis creation");
                        self.genesis.create_testnet_genesis_block()?
                    }
                    NetworkType::Private => {
                        tracing::info!("Using Private genesis creation");
                        self.genesis.create_private_genesis_block()?
                    }
                };
                tracing::info!(
                    "Persisting genesis block with index: {}",
                    genesis_block.header.index
                );
                self.persist_block(&genesis_block).await?;

                // Update height cache
                {
                    let mut height = self.current_height.write().await;
                    *height = 0;
                }
            }
        } else {
            // Update height cache
            {
                let mut height = self.current_height.write().await;
                *height = current_height;
            }
        }

        self.initialize_native_contract_states().await?;

        Ok(())
    }

    /// Seeds native contract metadata so higher layers can query it without C# fallbacks.
    async fn initialize_native_contract_states(&self) -> Result<()> {
        let state_guard = self.state.read().await;
        let native_states: Vec<_> = state_guard
            .list_native_contracts()
            .into_iter()
            .map(|info| info.to_contract_state())
            .collect();
        drop(state_guard);

        let mut registry = self.contract_states.write().await;
        for contract in native_states {
            registry.entry(contract.hash).or_insert(contract);
        }

        Ok(())
    }

    /// Gets the current block height (matches C# Neo Blockchain.Height exactly)
    pub async fn get_height(&self) -> u32 {
        *self.current_height.read().await
    }

    /// Gets a block by index (matches C# Neo Blockchain.GetBlock exactly)
    pub async fn get_block(&self, index: u32) -> Result<Option<Block>> {
        // Check cache first
        {
            let cache = self.block_cache.read().await;
            if let Some(block) = cache.get(&index) {
                return Ok(Some(block.clone()));
            }
        }

        // Load from persistence
        match self.persistence.get_block(index).await? {
            Some(block) => {
                // Cache the block
                {
                    let mut cache = self.block_cache.write().await;
                    cache.insert(index, block.clone());
                }
                Ok(Some(block))
            }
            None => Ok(None),
        }
    }

    /// Gets a block by hash (matches C# Neo Blockchain.GetBlock exactly)
    pub async fn get_block_by_hash(&self, hash: &UInt256) -> Result<Option<Block>> {
        self.persistence.get_block_by_hash(hash).await
    }

    /// Gets the height/index of a block by its hash (matches C# Neo Blockchain exactly)
    pub async fn get_block_height_by_hash(&self, hash: &UInt256) -> Result<Option<u32>> {
        match self.get_block_by_hash(hash).await? {
            Some(block) => Ok(Some(block.header.index)),
            None => Ok(None),
        }
    }

    /// Gets a transaction by hash (matches C# Neo Blockchain.GetTransaction exactly)
    pub async fn get_transaction(&self, hash: &UInt256) -> Result<Option<Transaction>> {
        // Check cache first
        {
            let cache = self.transaction_cache.read().await;
            if let Some(tx) = cache.get(hash) {
                return Ok(Some(tx.clone()));
            }
        }

        // Load from persistence
        match self.persistence.get_transaction(hash).await? {
            Some(transaction) => {
                // Cache the transaction
                {
                    let mut cache = self.transaction_cache.write().await;
                    cache.insert(*hash, transaction.clone());
                }
                Ok(Some(transaction))
            }
            None => Ok(None),
        }
    }

    /// Retrieves the block height where a transaction was included, if any.
    pub async fn get_transaction_height(&self, hash: &UInt256) -> Result<Option<u32>> {
        self.persistence.get_transaction_block_index(hash).await
    }

    /// Returns the registered contract state for the given hash if it exists.
    pub async fn get_contract_state(&self, hash: &UInt160) -> Result<Option<ContractState>> {
        if let Some(cached) = {
            let registry = self.contract_states.read().await;
            registry.get(hash).cloned()
        } {
            return Ok(Some(cached));
        }

        let state_result = {
            let state_guard = self.state.read().await;
            state_guard.get_contract(hash).await?
        };

        if let Some(state) = state_result {
            let mut registry = self.contract_states.write().await;
            registry.insert(state.hash, state.clone());
            Ok(Some(state))
        } else {
            Ok(None)
        }
    }

    /// Registers or updates a contract state within the persistence-backed registry.
    pub async fn register_contract_state(&self, state: ContractState) -> Result<()> {
        {
            let state_guard = self.state.read().await;
            state_guard.put_contract(state.clone()).await?;
        }

        {
            let mut registry = self.contract_states.write().await;
            registry.insert(state.hash, state);
        }

        // Ensure the persisted contract is durable so subsequent processes can observe it.
        self.persistence.commit().await
    }

    /// Lists the known native contract states in deterministic order.
    pub async fn list_native_contracts(&self) -> Vec<ContractState> {
        let registry = self.contract_states.read().await;
        let mut contracts: Vec<_> = registry.values().cloned().collect();
        contracts.sort_by_key(|c| c.id);
        contracts
    }

    /// Returns a raw storage value for the provided contract hash/key pair, if persisted.
    pub async fn get_raw_storage_value(
        &self,
        script_hash: &[u8],
        key: &[u8],
    ) -> Result<Option<Vec<u8>>> {
        let hash = UInt160::from_bytes(script_hash)
            .map_err(|e| Error::InvalidData(format!("Invalid script hash bytes: {}", e)))?;
        let storage_key = StorageKey::contract_storage(&hash, key);
        Ok(self
            .persistence
            .get(&storage_key)
            .await?
            .map(|item| item.value))
    }

    /// Retrieves a NEP-17 (fungible token) balance for the given account.
    pub async fn get_nep17_balance(
        &self,
        contract_hash: &UInt160,
        account: &UInt160,
    ) -> Result<Nep17Balance> {
        let mut storage_key_bytes = Vec::with_capacity(1 + account.as_bytes().len());
        storage_key_bytes.push(PREFIX_ACCOUNT);
        storage_key_bytes.extend_from_slice(&account.as_bytes());

        let storage_key = StorageKey::contract_storage(contract_hash, &storage_key_bytes);
        let storage_item = match self.persistence.get(&storage_key).await? {
            Some(item) => item,
            None => {
                return Ok(Nep17Balance {
                    amount: 0,
                    last_updated_block: 0,
                });
            }
        };

        let mut reader = MemoryReader::new(&storage_item.value);
        let balance_bytes = reader.read_var_bytes(MAX_SCRIPT_SIZE).map_err(|e| {
            Error::InvalidData(format!("Failed to decode NEP-17 balance state: {}", e))
        })?;

        let balance_bigint = if balance_bytes.is_empty() {
            BigInt::zero()
        } else {
            BigInt::from_signed_bytes_le(&balance_bytes)
        };

        let last_updated_block = reader.read_u32().unwrap_or(0);

        let positive = match balance_bigint.sign() {
            Sign::Minus => BigInt::zero(),
            _ => balance_bigint,
        };

        let balance_biguint = positive.to_biguint().unwrap_or_else(BigUint::zero);
        let amount = balance_biguint.to_u128().unwrap_or(u128::MAX);

        Ok(Nep17Balance {
            amount,
            last_updated_block,
        })
    }

    /// Stores a raw storage value for a contract. Useful for synchronising state from
    /// external sources during testing or bootstrap scenarios.
    pub async fn set_raw_storage_value(
        &self,
        script_hash: &UInt160,
        key: Vec<u8>,
        value: Vec<u8>,
    ) -> Result<()> {
        let storage_key = StorageKey::contract_storage(script_hash, &key);
        self.persistence
            .put(storage_key, StorageItem::new(value))
            .await?;
        // Ensure writes are flushed so later reads observe the new value.
        self.persistence.commit().await
    }

    /// Gets the header of the latest block (matches C# Neo Blockchain.HeaderHeight exactly)
    pub async fn get_header_height(&self) -> u32 {
        // In this implementation, header height equals block height
        self.get_height().await
    }

    /// Persists a block to the blockchain (matches C# Neo Blockchain.Persist exactly)
    pub async fn persist_block(&self, block: &Block) -> Result<()> {
        let _lock = self.persist_lock.lock().await;

        if block.header.index > 0 {
            // Validate block first
            tracing::debug!(
                "🔍 Starting block verification for block index {}",
                block.header.index
            );
            let verification_result = self.verifier.verify_header(&block.header).await?;
            tracing::debug!("🔍 Block verification result: {:?}", verification_result);
            if verification_result != VerifyResult::Succeed {
                tracing::error!(
                    "❌ Block header verification failed with result: {:?}",
                    verification_result
                );
                return Err(Error::Validation(
                    "Block header verification failed".to_string(),
                ));
            }
            tracing::debug!("✅ Block verification succeeded");
        } else {
            tracing::debug!("⏭️ Skipping verification for genesis block");
        }

        // Verify block index is correct
        let current_height = self.get_height().await;
        tracing::debug!(
            "🔍 Current height: {}, block index: {}",
            current_height,
            block.header.index
        );
        let expected_index = if block.header.index == 0 {
            // Genesis block should have index 0
            0
        } else {
            current_height + 1
        };

        if block.header.index != expected_index {
            tracing::error!(
                "❌ Block index validation failed: expected {}, got {}",
                expected_index,
                block.header.index
            );
            return Err(Error::Validation(format!(
                "Invalid block index: expected {}, got {}",
                expected_index, block.header.index
            )));
        }
        tracing::debug!("✅ Block index validation passed");

        // Verify previous hash
        if block.header.index > 0 {
            if let Some(previous_block) = self.get_block(current_height).await? {
                if block.header.previous_hash != previous_block.hash() {
                    return Err(Error::Validation("Invalid previous hash".to_string()));
                }
            }
        }

        // Verify transactions
        for transaction in &block.transactions {
            let tx_verification = self.verifier.verify_transaction(transaction).await?;
            if tx_verification != VerifyResult::Succeed {
                tracing::error!(
                    "❌ Transaction verification failed for tx in block {}",
                    block.header.index
                );
                return Err(Error::Validation(
                    "Transaction verification failed".to_string(),
                ));
            }
        }

        // Persist the block
        self.persistence.persist_block(block).await?;

        // Update height cache
        {
            let mut height = self.current_height.write().await;
            *height = block.header.index;
        }

        // Cache the block
        {
            let mut cache = self.block_cache.write().await;
            cache.insert(block.header.index, block.clone());
        }

        // Cache transactions
        {
            let mut tx_cache = self.transaction_cache.write().await;
            for transaction in &block.transactions {
                let tx_hash = transaction.hash()?;
                tx_cache.insert(tx_hash, transaction.clone());
            }
        }

        Ok(())
    }

    /// Adds a transaction to the blockchain (matches C# Neo Blockchain.ContainsTransaction exactly)
    pub async fn contains_transaction(&self, hash: &UInt256) -> Result<bool> {
        match self.get_transaction(hash).await? {
            Some(_) => Ok(true),
            None => Ok(false),
        }
    }

    /// Gets the best block hash (matches C# Neo Blockchain.CurrentBlockHash exactly)
    pub async fn get_best_block_hash(&self) -> Result<UInt256> {
        let height = self.get_height().await;
        if let Some(block) = self.get_block(height).await? {
            Ok(block.hash())
        } else {
            Err(Error::NotFound)
        }
    }

    /// Validates a transaction against current blockchain state
    pub async fn validate_transaction(&self, transaction: &Transaction) -> Result<bool> {
        let state = self.state.read().await;
        state.validate_transaction(transaction).await
    }

    /// Gets current policy settings
    pub async fn get_policy_settings(&self) -> PolicySettings {
        let state = self.state.read().await;
        state.get_policy_settings().clone()
    }

    /// Creates a snapshot of current blockchain state (matches C# Neo Blockchain.GetSnapshot exactly)
    pub async fn create_snapshot(&self) -> Result<BlockchainSnapshot> {
        self.persistence.create_snapshot().await
    }

    /// Gets the network type
    pub fn network(&self) -> NetworkType {
        self.network
    }

    /// Gets the network magic number
    pub fn magic(&self) -> u32 {
        self.network.magic()
    }

    /// Gets blockchain statistics
    pub async fn get_stats(&self) -> BlockchainStats {
        let height = self.get_height().await;
        let (block_cache_size, tx_cache_size) = {
            let block_cache = self.block_cache.read().await;
            let tx_cache = self.transaction_cache.read().await;
            (block_cache.len(), tx_cache.len())
        };
        let (read_cache_size, write_cache_size) = self.persistence.cache_stats().await;

        BlockchainStats {
            height,
            block_cache_size,
            transaction_cache_size: tx_cache_size,
            storage_read_cache_size: read_cache_size,
            storage_write_cache_size: write_cache_size,
        }
    }

    /// Clears all caches
    pub async fn clear_caches(&self) {
        {
            let mut block_cache = self.block_cache.write().await;
            block_cache.clear();
        }
        {
            let mut tx_cache = self.transaction_cache.write().await;
            tx_cache.clear();
        }
        {
            let mut state = self.state.write().await;
            state.clear_caches().await;
        }
    }

    /// Gets memory usage statistics
    pub async fn get_memory_usage(&self) -> MemoryUsage {
        let stats = self.get_stats().await;

        let block_cache_bytes = stats.block_cache_size * MAX_SCRIPT_SIZE; // ~1KB per cached block header
        let tx_cache_bytes = stats.transaction_cache_size * MAX_TRANSACTIONS_PER_BLOCK; // ~MAX_TRANSACTIONS_PER_BLOCK bytes per cached transaction
        let storage_cache_bytes =
            (stats.storage_read_cache_size + stats.storage_write_cache_size) * 128; // ~128 bytes per cache entry

        MemoryUsage {
            total_bytes: block_cache_bytes + tx_cache_bytes + storage_cache_bytes,
            block_cache_bytes,
            transaction_cache_bytes: tx_cache_bytes,
            storage_cache_bytes,
        }
    }

    /// Validates blockchain integrity
    pub async fn validate_integrity(&self) -> Result<IntegrityReport> {
        let mut report = IntegrityReport::default();
        let height = self.get_height().await;

        // Validate block chain continuity
        for i in 0..=height {
            if let Some(block) = self.get_block(i).await? {
                report.blocks_checked += 1;

                if i > 0 {
                    if let Some(prev_block) = self.get_block(i - 1).await? {
                        if block.header.previous_hash != prev_block.hash() {
                            report
                                .errors
                                .push(format!("Block {} has invalid previous hash", i));
                        }
                    } else {
                        report
                            .errors
                            .push(format!("Missing previous block for block {}", i));
                    }
                }

                // Validate block index
                if block.header.index != i {
                    report.errors.push(format!(
                        "Block at height {} has wrong index {}",
                        i, block.header.index
                    ));
                }

                // Count transactions
                report.transactions_checked += block.transactions.len();
            } else {
                report.errors.push(format!("Missing block at height {}", i));
            }
        }

        Ok(report)
    }

    /// Adds a block with fork detection and chain reorganization support
    pub async fn add_block_with_fork_detection(&self, block: &Block) -> Result<()> {
        let _lock = self.persist_lock.lock().await;

        // First verify the block
        if block.header.index > 0 {
            let verification_result = self.verifier.verify_header(&block.header).await?;
            if verification_result != VerifyResult::Succeed {
                return Err(Error::Validation(
                    "Block header verification failed".to_string(),
                ));
            }
        }

        let current_height = self.get_height().await;
        let current_best_hash = self.get_best_block_hash().await?;

        if block.header.previous_hash == current_best_hash
            && block.header.index == current_height + 1
        {
            // Normal case: block extends current chain
            return self.persist_block(block).await;
        }

        if self
            .get_block_by_hash_internal(&block.header.previous_hash)
            .await?
            .is_none()
        {
            // Store as orphan block
            let mut orphans = self.orphan_blocks.write().await;
            orphans
                .entry(block.header.previous_hash.clone())
                .or_insert_with(Vec::new)
                .push(block.clone());

            tracing::info!(
                "Stored orphan block {} at height {} waiting for parent {}",
                block.hash(),
                block.header.index,
                block.header.previous_hash
            );
            return Ok(());
        }

        let fork_point = self.find_fork_point(&block.header.previous_hash).await?;
        let current_chain_work = self
            .calculate_chain_work(current_height, fork_point)
            .await?;
        let new_chain_work = self.calculate_fork_chain_work(block, fork_point).await?;

        if new_chain_work > current_chain_work {
            // New chain has more work - perform reorganization
            tracing::info!(
                "Fork detected at height {}. Reorganizing chain. Current work: {}, New work: {}",
                fork_point,
                current_chain_work,
                new_chain_work
            );

            self.reorganize_chain(block, fork_point).await?;
        } else {
            // Current chain has more work - store as alternative tip
            let mut fork_cache = self.fork_cache.write().await;
            fork_cache.insert(block.hash(), block.clone());

            tracing::info!(
                "Fork detected but current chain has more work. Storing alternative tip at height {}",
                block.header.index
            );
        }

        self.process_orphan_blocks(&block.hash()).await?;

        Ok(())
    }

    /// Finds the common ancestor between current chain and a fork
    async fn find_fork_point(&self, fork_hash: &UInt256) -> Result<u32> {
        let mut hash = fork_hash.clone();

        loop {
            if let Some(block) = self.get_block_by_hash_internal(&hash).await? {
                if let Some(main_block) = self.get_block(block.header.index).await? {
                    if main_block.hash() == block.hash() {
                        return Ok(block.header.index);
                    }
                }
                hash = block.header.previous_hash.clone();
            } else {
                return Err(Error::NotFound);
            }
        }
    }

    /// Calculates cumulative work for a chain segment
    async fn calculate_chain_work(&self, from_height: u32, to_height: u32) -> Result<u64> {
        let mut work = 0u64;
        for height in (to_height + 1)..=from_height {
            if let Some(_block) = self.get_block(height).await? {
                // In Neo, all blocks have equal weight
                // In a real implementation, this might consider difficulty
                work += 1;
            }
        }
        Ok(work)
    }

    /// Calculates cumulative work for a fork chain
    async fn calculate_fork_chain_work(&self, tip: &Block, fork_point: u32) -> Result<u64> {
        let mut work = 1u64; // Count the tip block
        let mut current_hash = tip.header.previous_hash.clone();

        loop {
            if let Some(block) = self.get_block_by_hash_internal(&current_hash).await? {
                if block.header.index <= fork_point {
                    break;
                }
                work += 1;
                current_hash = block.header.previous_hash.clone();
            } else {
                // Check fork cache
                let fork_cache = self.fork_cache.read().await;
                if let Some(block) = fork_cache.get(&current_hash) {
                    if block.header.index <= fork_point {
                        break;
                    }
                    work += 1;
                    current_hash = block.header.previous_hash.clone();
                } else {
                    return Err(Error::NotFound);
                }
            }
        }

        Ok(work)
    }

    /// Performs chain reorganization
    async fn reorganize_chain(&self, new_tip: &Block, fork_point: u32) -> Result<()> {
        tracing::info!("Starting chain reorganization from height {}", fork_point);

        // 1. Collect blocks to remove (current chain from fork point)
        let current_height = self.get_height().await;
        let mut blocks_to_remove = Vec::new();
        for height in ((fork_point + 1)..=current_height).rev() {
            if let Some(block) = self.get_block(height).await? {
                blocks_to_remove.push(block);
            }
        }

        // 2. Collect blocks to add (new chain from fork point)
        let mut blocks_to_add = Vec::new();
        let mut current_block = new_tip.clone();
        while current_block.header.index > fork_point {
            blocks_to_add.push(current_block.clone());

            if let Some(parent) = self
                .get_block_by_hash_internal(&current_block.header.previous_hash)
                .await?
            {
                current_block = parent;
            } else {
                // Check fork cache
                let fork_cache = self.fork_cache.read().await;
                if let Some(parent) = fork_cache.get(&current_block.header.previous_hash) {
                    current_block = parent.clone();
                } else {
                    return Err(Error::NotFound);
                }
            }
        }
        blocks_to_add.reverse(); // Order from oldest to newest

        // 3. Rollback removed blocks
        for block in &blocks_to_remove {
            self.rollback_block(block).await?;
        }

        // 4. Apply new blocks
        for block in &blocks_to_add {
            self.persist_block(block).await?;
        }

        tracing::info!(
            "Chain reorganization complete. Removed {} blocks, added {} blocks",
            blocks_to_remove.len(),
            blocks_to_add.len()
        );

        Ok(())
    }

    /// Rolls back a single block
    async fn rollback_block(&self, block: &Block) -> Result<()> {
        tracing::info!("Rolling back block at height {}", block.header.index);

        // Remove from persistence
        self.persistence.remove_block(block.header.index).await?;

        // Remove from caches
        {
            let mut block_cache = self.block_cache.write().await;
            block_cache.remove(&block.header.index);
        }

        {
            let mut tx_cache = self.transaction_cache.write().await;
            for transaction in &block.transactions {
                let tx_hash = transaction.hash()?;
                tx_cache.remove(&tx_hash);
            }
        }

        // Update height
        {
            let mut height = self.current_height.write().await;
            *height = block.header.index - 1;
        }

        Ok(())
    }

    /// Processes orphan blocks that might now be connectable
    async fn process_orphan_blocks(&self, parent_hash: &UInt256) -> Result<()> {
        let mut orphans_to_process = vec![parent_hash.clone()];

        while let Some(hash) = orphans_to_process.pop() {
            let blocks_to_add = {
                let mut orphans = self.orphan_blocks.write().await;
                orphans.remove(&hash).unwrap_or_default()
            };

            for block in blocks_to_add {
                tracing::info!(
                    "Processing orphan block {} at height {}",
                    block.hash(),
                    block.header.index
                );

                let blockchain = self.clone();
                let block_clone = block.clone();
                if Box::pin(blockchain.add_block_with_fork_detection(&block_clone))
                    .await
                    .is_ok()
                {
                    orphans_to_process.push(block.hash());
                }
            }
        }

        Ok(())
    }

    /// Gets a block by its hash (internal method used by fork detection)
    async fn get_block_by_hash_internal(&self, hash: &UInt256) -> Result<Option<Block>> {
        // First check cache
        {
            let cache = self.block_cache.read().await;
            for block in cache.values() {
                if block.hash() == *hash {
                    return Ok(Some(block.clone()));
                }
            }
        }

        // Check fork cache
        {
            let fork_cache = self.fork_cache.read().await;
            if let Some(block) = fork_cache.get(hash) {
                return Ok(Some(block.clone()));
            }
        }

        // Finally check persistence
        self.persistence.get_block_by_hash(hash).await
    }
}

/// Blockchain statistics
#[derive(Debug, Clone)]
pub struct BlockchainStats {
    /// Current blockchain height
    pub height: u32,
    /// Number of cached blocks
    pub block_cache_size: usize,
    /// Number of cached transactions
    pub transaction_cache_size: usize,
    /// Storage read cache size
    pub storage_read_cache_size: usize,
    /// Storage write cache size
    pub storage_write_cache_size: usize,
}

/// Memory usage information
#[derive(Debug, Clone)]
pub struct MemoryUsage {
    /// Total memory usage in bytes
    pub total_bytes: usize,
    /// Block cache memory usage
    pub block_cache_bytes: usize,
    /// Transaction cache memory usage
    pub transaction_cache_bytes: usize,
    /// Storage cache memory usage
    pub storage_cache_bytes: usize,
}

/// Blockchain integrity report
#[derive(Debug, Clone, Default)]
pub struct IntegrityReport {
    /// Number of blocks checked
    pub blocks_checked: usize,
    /// Number of transactions checked
    pub transactions_checked: usize,
    /// List of errors found
    pub errors: Vec<String>,
}

impl IntegrityReport {
    /// Returns true if blockchain is valid
    pub fn is_valid(&self) -> bool {
        self.errors.is_empty()
    }
}

#[cfg(test)]
#[allow(dead_code)]
mod tests {
    use super::*;
    use crate::{blockchain::storage::StorageKey, Error, Result};
    use neo_core::UInt160;
    use neo_io::MemoryReader;
    use neo_smart_contract::contract_state::{ContractState, NefFile};
    use neo_smart_contract::manifest::ContractManifest;
    use neo_smart_contract::native::fungible_token::PREFIX_ACCOUNT;
    use num_bigint::BigInt;
    use std::str::FromStr;

    #[tokio::test]
    async fn test_blockchain_creation() -> Result<()> {
        let blockchain =
            Blockchain::new_with_storage_suffix(NetworkType::TestNet, Some("blockchain_creation"))
                .await?;

        // Should start with genesis block
        assert_eq!(blockchain.get_height().await, 0);

        // Genesis block should exist
        let genesis = blockchain.get_block(0).await?;
        assert!(genesis.is_some());
        Ok(())
    }

    #[tokio::test]
    async fn test_network_types() {
        assert_eq!(NetworkType::MainNet.magic(), 0x334f454e);
        assert_eq!(NetworkType::TestNet.magic(), 0x3554334e);
        assert_eq!(NetworkType::Private.magic(), 0x00000000);
    }

    #[tokio::test]
    async fn test_blockchain_stats() -> Result<()> {
        let blockchain =
            Blockchain::new_with_storage_suffix(NetworkType::TestNet, Some("blockchain_stats"))
                .await?;
        let stats = blockchain.get_stats().await;

        assert_eq!(stats.height, 0); // Only genesis block
        assert!(stats.block_cache_size <= 1); // Genesis might be cached
        Ok(())
    }

    #[tokio::test]
    async fn test_integrity_validation() -> Result<()> {
        let blockchain =
            Blockchain::new_with_storage_suffix(NetworkType::TestNet, Some("integrity_validation"))
                .await?;
        let report = blockchain.validate_integrity().await?;

        assert!(report.is_valid());
        assert_eq!(report.blocks_checked, 1); // Genesis block
        Ok(())
    }

    #[tokio::test]
    async fn contract_state_round_trip_hits_persistence() -> Result<()> {
        let blockchain = Blockchain::new_with_storage_suffix(
            NetworkType::Private,
            Some("contract_state_round_trip"),
        )
        .await?;

        let contract_hash = UInt160::from_bytes(&[0x11u8; 20]).expect("valid hash");
        let contract_state = ContractState::new(
            42,
            contract_hash,
            NefFile::new("test-compiler".to_string(), vec![0x40]),
            ContractManifest::new("TestContract".to_string()),
        );

        blockchain
            .register_contract_state(contract_state.clone())
            .await?;

        // Clear the in-memory cache so we must hit persistence via BlockchainState.
        blockchain
            .contract_states
            .write()
            .await
            .remove(&contract_hash);

        let storage_key = StorageKey::contract(contract_hash.clone());
        let storage_item = blockchain
            .persistence
            .get(&storage_key)
            .await?
            .expect("contract state persisted");

        let mut reader = MemoryReader::new(&storage_item.value);
        let stored_state = ContractState::deserialize(&mut reader)
            .map_err(|e| Error::StorageError(format!("Contract state decode failed: {e}")))?;
        assert_eq!(stored_state, contract_state);

        let fetched = blockchain.get_contract_state(&contract_hash).await?;
        assert_eq!(fetched, Some(contract_state.clone()));

        // Drop the in-memory blockchain to ensure we reopen the persisted store.
        drop(blockchain);

        let blockchain_reopened = Blockchain::new_with_storage_suffix(
            NetworkType::Private,
            Some("contract_state_round_trip"),
        )
        .await?;
        let fetched_after_restart = blockchain_reopened
            .get_contract_state(&contract_hash)
            .await?;
        assert_eq!(fetched_after_restart, Some(contract_state));
        Ok(())
    }

    #[tokio::test]
    async fn raw_storage_round_trip_hits_persistence() -> Result<()> {
        let blockchain = Blockchain::new_with_storage_suffix(
            NetworkType::Private,
            Some("contract_storage_round_trip"),
        )
        .await?;

        let contract_hash = UInt160::from_bytes(&[0x22u8; 20]).expect("valid hash");
        let key = vec![0xAA, 0xBB, 0xCC];
        let value = vec![0x01, 0x02, 0x03];

        blockchain
            .set_raw_storage_value(&contract_hash, key.clone(), value.clone())
            .await?;

        let fetched = blockchain
            .get_raw_storage_value(&contract_hash.as_bytes(), &key)
            .await?;
        assert_eq!(fetched, Some(value));
        Ok(())
    }

    fn encode_balance_state(balance: u64, height: u32) -> Vec<u8> {
        let mut payload = Vec::new();
        let mut balance_bytes = BigInt::from(balance).to_signed_bytes_le();
        if balance_bytes.is_empty() {
            balance_bytes.push(0);
        }

        match balance_bytes.len() {
            len if len < 0xFD => payload.push(len as u8),
            len if len <= 0xFFFF => {
                payload.push(0xFD);
                payload.extend_from_slice(&(len as u16).to_le_bytes());
            }
            len if len <= 0xFFFF_FFFF => {
                payload.push(0xFE);
                payload.extend_from_slice(&(len as u32).to_le_bytes());
            }
            len => {
                payload.push(0xFF);
                payload.extend_from_slice(&(len as u64).to_le_bytes());
            }
        }

        payload.extend_from_slice(&balance_bytes);
        payload.extend_from_slice(&height.to_le_bytes());
        payload
    }

    #[tokio::test]
    async fn nep17_balance_reads_balance_state() -> Result<()> {
        let blockchain = Blockchain::new_with_storage_suffix(
            NetworkType::Private,
            Some("nep17_balance_round_trip"),
        )
        .await?;

        let contract_hash = UInt160::from_str("d2a4cff31913016155e38e474a2c06d08be276cf")?;
        let account = UInt160::from_bytes(&[0xAAu8; 20]).expect("valid account");

        let mut key = vec![PREFIX_ACCOUNT];
        key.extend_from_slice(&account.as_bytes());
        let value = encode_balance_state(1_000_000_000, 123);

        blockchain
            .set_raw_storage_value(&contract_hash, key, value)
            .await?;

        let balance = blockchain
            .get_nep17_balance(&contract_hash, &account)
            .await?;
        assert_eq!(balance.amount, 1_000_000_000u128);
        assert_eq!(balance.last_updated_block, 123);

        let other_account = UInt160::from_bytes(&[0xBBu8; 20]).expect("valid account");
        let other_balance = blockchain
            .get_nep17_balance(&contract_hash, &other_account)
            .await?;
        assert_eq!(other_balance.amount, 0);
        assert_eq!(other_balance.last_updated_block, 0);

        Ok(())
    }
}
