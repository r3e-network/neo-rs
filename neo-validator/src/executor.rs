//! Real neo-rs block execution and incremental state-root verification.

use crate::upstream::ExecutionReceipt;
use anyhow::{Context, Result, bail};
use neo_native_contracts::ledger_contract::{HashOrIndex, LedgerContract};
use neo_primitives::{TriggerType, UInt256};
use neo_protocol::ProtocolSettings;
use neo_protocol::execution::ApplicationExecuted;
use neo_protocol::payloads::Block;
use neo_storage::persistence::store_provider::StoreProvider;
use neo_system::state_service::{
    StateRootNodeCacheStats, StateServiceCommitHandlers, StateServiceSettings,
    UnhandledExceptionPolicy,
};
use neo_system::{ExecutionMode, ExecutionProfileSnapshot, NeoSystem};
use std::path::Path;
use std::sync::Arc;

#[derive(Debug)]
pub struct ExecutionOutcome {
    pub block_hash: UInt256,
    pub state_root: UInt256,
    /// `None` means the block was already atomically persisted before a restart.
    /// Remote receipts are still compared with each other, and the persisted
    /// local state root is always verified again.
    pub executions: Option<Vec<ExecutionReceipt>>,
}

pub trait BlockExecutor: Send + Sync {
    fn height(&self) -> Result<Option<u32>>;
    fn checkpoint(&self) -> Result<Option<crate::state::VerifiedPoint>>;
    fn execute(&self, block: Block) -> Result<ExecutionOutcome>;
}

pub struct NeoExecutor {
    system: Arc<NeoSystem>,
    state_handlers: Arc<StateServiceCommitHandlers>,
    /// Set after any speculative empty-block batch failure. Physical storage may
    /// still be at the pre-batch checkpoint while committed callbacks advanced
    /// process-local MPT/root state, so only a fresh executor may continue.
    poisoned: std::sync::atomic::AtomicBool,
}

impl NeoExecutor {
    #[cfg(feature = "rocksdb")]
    pub fn open(settings: ProtocolSettings, data_path: &Path) -> Result<Self> {
        use neo_storage::persistence::providers::rocksdb::RocksDBStoreProvider;
        use neo_storage::persistence::storage::StorageConfig;

        let config = StorageConfig {
            path: data_path.to_path_buf(),
            ..StorageConfig::default()
        };
        let provider: Arc<dyn StoreProvider> = Arc::new(RocksDBStoreProvider::new(config));
        Self::open_with_provider(settings, provider, data_path)
    }

    /// Opens offline replay storage with either per-block or per-batch WAL sync.
    /// Batch mode keeps WAL enabled and requires `flush_durable` before each
    /// corresponding audit batch is flushed or reported as verified.
    #[cfg(feature = "rocksdb")]
    pub fn open_with_batch_durability(
        settings: ProtocolSettings,
        data_path: &Path,
        batch_durable: bool,
    ) -> Result<Self> {
        use neo_storage::persistence::providers::rocksdb::{
            BatchCommitConfig, RocksDBStoreProvider,
        };
        use neo_storage::persistence::storage::StorageConfig;

        let config = StorageConfig {
            path: data_path.to_path_buf(),
            ..StorageConfig::default()
        };
        let mut batch_config = BatchCommitConfig::durable();
        if batch_durable {
            batch_config.sync_on_flush = false;
            batch_config.disable_wal = false;
        }
        let provider: Arc<dyn StoreProvider> =
            Arc::new(RocksDBStoreProvider::new(config).with_batch_config(batch_config));
        Self::open_with_provider(settings, provider, data_path)
    }

    /// Opens checkpoint-gated offline replay storage without full historical hydration.
    #[cfg(feature = "rocksdb")]
    pub fn open_with_batch_durability_checkpoint_replay(
        settings: ProtocolSettings,
        data_path: &Path,
        batch_durable: bool,
    ) -> Result<Self> {
        use neo_storage::persistence::providers::rocksdb::{
            BatchCommitConfig, RocksDBStoreProvider,
        };
        use neo_storage::persistence::storage::StorageConfig;

        let config = StorageConfig {
            path: data_path.to_path_buf(),
            ..StorageConfig::default()
        };
        let mut batch_config = BatchCommitConfig::durable();
        if batch_durable {
            batch_config.sync_on_flush = false;
            batch_config.disable_wal = false;
        }
        let provider: Arc<dyn StoreProvider> =
            Arc::new(RocksDBStoreProvider::new(config).with_batch_config(batch_config));
        Self::open_with_provider_and_hydration(settings, provider, data_path, true)
    }

    #[cfg(not(feature = "rocksdb"))]
    pub fn open(_settings: ProtocolSettings, _data_path: &Path) -> Result<Self> {
        bail!("neo-validator was built without the required rocksdb feature")
    }

    pub fn open_with_provider(
        settings: ProtocolSettings,
        provider: Arc<dyn StoreProvider>,
        data_path: &Path,
    ) -> Result<Self> {
        Self::open_with_provider_and_hydration(settings, provider, data_path, false)
    }

    fn open_with_provider_and_hydration(
        settings: ProtocolSettings,
        provider: Arc<dyn StoreProvider>,
        data_path: &Path,
        checkpoint_tip: bool,
    ) -> Result<Self> {
        std::fs::create_dir_all(data_path).with_context(|| {
            format!("failed to create validator storage {}", data_path.display())
        })?;
        let state_settings = StateServiceSettings {
            full_state: true,
            path: data_path.join("state").to_string_lossy().into_owned(),
            network: settings.network,
            auto_verify: false,
            max_find_result_items: 100,
            exception_policy: UnhandledExceptionPolicy::StopNode,
        };
        let system = if checkpoint_tip {
            NeoSystem::new_with_state_service_deferring_genesis_checkpoint_replay(
                settings,
                Some(provider),
                Some(data_path.to_string_lossy().into_owned()),
                Some(state_settings),
            )
        } else {
            NeoSystem::new_with_state_service_deferring_genesis(
                settings,
                Some(provider),
                Some(data_path.to_string_lossy().into_owned()),
                Some(state_settings),
            )
        }
        .map_err(|error| anyhow::anyhow!("failed to open neo-rs validator system: {error}"))?;
        let state_handlers = system
            .state_commit_handlers()
            .ok_or_else(|| anyhow::anyhow!("state-root commit handler was not registered"))?;
        Ok(Self {
            system,
            state_handlers,
            poisoned: std::sync::atomic::AtomicBool::new(false),
        })
    }

    fn ensure_live(&self) -> Result<()> {
        if self.poisoned.load(std::sync::atomic::Ordering::Acquire) {
            bail!(
                "validator executor is poisoned after an empty-block batch failure; reopen storage"
            )
        }
        Ok(())
    }

    /// Executes a bounded run of empty blocks with one final atomic database commit.
    /// No outcome is returned until every root record is readable from global storage.
    pub fn execute_empty_blocks(&self, mut blocks: Vec<Block>) -> Result<Vec<ExecutionOutcome>> {
        self.ensure_live()?;
        if !(2..=NeoSystem::MAX_EMPTY_BLOCK_WRITE_BACK_BLOCKS).contains(&blocks.len())
            || blocks.iter().any(|block| !block.transactions.is_empty())
        {
            bail!(
                "empty-block execution requires 2..={} transaction-free blocks",
                NeoSystem::MAX_EMPTY_BLOCK_WRITE_BACK_BLOCKS
            );
        }

        // Reject malformed input before starting speculative execution. These
        // validation failures do not poison an otherwise unchanged executor.
        let mut expected_height = self.height()?.map_or(0, |height| {
            height.checked_add(1).expect("block height overflow")
        });
        let mut previous_hash = None;
        let mut block_hashes = Vec::with_capacity(blocks.len());
        for block in &mut blocks {
            if block.index() != expected_height {
                bail!(
                    "non-contiguous empty block {}; expected {expected_height}",
                    block.index()
                );
            }
            if previous_hash
                .as_ref()
                .is_some_and(|hash| block.prev_hash() != hash)
            {
                bail!(
                    "empty block {} does not extend its predecessor",
                    block.index()
                );
            }
            expected_height = expected_height
                .checked_add(1)
                .expect("block height overflow");
            previous_hash = Some(block.hash());
            block_hashes.push(previous_hash.expect("hash was just assigned"));
        }

        let result = (|| {
            let executions = self
                .system
                .persist_consecutive_empty_blocks_without_runtime_cache(blocks.clone())
                .map_err(|error| anyhow::anyhow!("neo-rs empty-block batch failed: {error}"))?;
            if executions.len() != blocks.len() {
                bail!("empty-block batch returned the wrong receipt count");
            }
            if self.height()? != blocks.last().map(neo_ledger::Block::index) {
                bail!("empty-block batch did not advance persistent height to its final block");
            }

            let mut outcomes = Vec::with_capacity(blocks.len());
            for ((block, block_hash), executed) in blocks.iter().zip(block_hashes).zip(executions) {
                let state_root = self
                    .state_handlers
                    .get_state_root(block.index())
                    .map(|root| root.root_hash)
                    .ok_or_else(|| {
                        anyhow::anyhow!("empty-block batch did not persist root {}", block.index())
                    })?;
                outcomes.push(ExecutionOutcome {
                    block_hash,
                    state_root,
                    executions: Some(local_receipts(&executed)),
                });
            }
            if self.state_handlers.current_root_hash()
                != outcomes.last().map(|outcome| outcome.state_root)
            {
                bail!("empty-block batch final in-memory and persisted roots differ");
            }
            Ok(outcomes)
        })();
        if result.is_err() {
            self.poisoned
                .store(true, std::sync::atomic::Ordering::Release);
        }
        result
    }

    /// Selects canonical, automatic direct-native, or dual-execution verification.
    pub fn set_execution_mode(&self, mode: ExecutionMode) {
        self.system.set_execution_mode(mode);
    }

    /// Enables optional wall-clock profiling without changing the execution pipeline.
    pub fn set_execution_profiling(&self, enabled: bool) {
        let stats = self.system.execution_stats();
        stats.set_profiling(enabled);
        if enabled {
            let profile_stats = Arc::clone(&stats);
            let sink: neo_system::state_service::StateRootProfileSink = Arc::new(move |profile| {
                profile_stats.record_state_root_profile(
                    profile.change_extraction,
                    profile.mpt,
                    profile.mpt_detail,
                );
            });
            self.state_handlers.set_profile_sink(Some(sink));
        } else {
            self.state_handlers.set_profile_sink(None);
        }
    }

    /// Enables or bypasses MPT compact-node caching for offline A/B profiling.
    pub fn set_mpt_node_cache_enabled(&self, enabled: bool) {
        self.state_handlers.set_node_cache_enabled(enabled);
    }

    /// Starts a cold MPT-cache experiment with an explicit nonzero capacity.
    /// Call before replay begins; persistent nodes and roots are not modified.
    pub fn reset_mpt_node_cache_capacity(&self, capacity: std::num::NonZeroUsize) {
        self.state_handlers.reset_node_cache_capacity(capacity);
    }

    /// Returns store-scoped MPT cache activity for the current process.
    pub fn mpt_node_cache_stats(&self) -> StateRootNodeCacheStats {
        self.state_handlers.node_cache_stats()
    }

    /// Returns aggregate timings since profiling was enabled.
    pub fn execution_profile(&self) -> ExecutionProfileSnapshot {
        self.system.execution_stats().profile_snapshot()
    }

    /// Synchronizes all accepted writes before a batch audit watermark is flushed.
    pub fn flush_durable(&self) -> Result<()> {
        self.system
            .store()
            .try_flush_durable()
            .map_err(|error| anyhow::anyhow!("durable storage flush failed: {error}"))
    }

    /// Returns the number of transactions actually executed by direct native dispatch.
    pub fn direct_transfer_count(&self) -> u64 {
        self.system.execution_stats().direct_transfer_count()
    }

    /// Returns native-contract state entries for bounded differential diagnostics.
    /// Ledger bookkeeping and state-service internals are deliberately excluded,
    /// matching the storage domain covered by Neo's state-root MPT.
    pub fn native_contract_storage(&self) -> Vec<(i32, Vec<u8>, Vec<u8>)> {
        self.system
            .store_cache()
            .find(
                None,
                neo_storage::persistence::seek_direction::SeekDirection::Forward,
            )
            .filter(|(key, _)| (-100..0).contains(&key.id) && key.id != -4)
            .map(|(key, item)| (key.id, key.key().to_vec(), item.value_bytes().into_owned()))
            .collect()
    }

    pub async fn shutdown(&self) -> Result<()> {
        self.system
            .clone()
            .shutdown()
            .await
            .map_err(|error| anyhow::anyhow!("validator system shutdown failed: {error}"))
    }
}

impl BlockExecutor for NeoExecutor {
    fn height(&self) -> Result<Option<u32>> {
        self.ensure_live()?;
        let cache = self.system.store_cache();
        let ledger = LedgerContract::new();
        let genesis = neo_ledger::create_genesis_block(self.system.settings()).hash();
        if !ledger.contains_block(&cache, &genesis) {
            return Ok(None);
        }
        ledger
            .current_index(&cache)
            .map(Some)
            .map_err(|error| anyhow::anyhow!("failed to read validator height: {error}"))
    }

    fn checkpoint(&self) -> Result<Option<crate::state::VerifiedPoint>> {
        let Some(height) = self.height()? else {
            return Ok(None);
        };
        let cache = self.system.store_cache();
        let block = LedgerContract::new()
            .get_block(&cache, HashOrIndex::Index(height))
            .map_err(|error| anyhow::anyhow!("failed to read checkpoint block: {error}"))?
            .ok_or_else(|| anyhow::anyhow!("checkpoint block is missing"))?;
        let state_root = self
            .state_handlers
            .get_state_root(height)
            .map(|root| root.root_hash)
            .ok_or_else(|| anyhow::anyhow!("checkpoint state root is missing"))?;
        Ok(Some(crate::state::VerifiedPoint {
            height,
            block_hash: block.hash(),
            state_root,
        }))
    }

    fn execute(&self, mut block: Block) -> Result<ExecutionOutcome> {
        self.ensure_live()?;
        let height = block.index();
        let block_hash = block.hash();
        let cache = self.system.store_cache();
        let ledger = LedgerContract::new();
        let existing = ledger
            .get_block(&cache, HashOrIndex::Index(height))
            .map_err(|error| anyhow::anyhow!("failed to read local block: {error}"))?;

        let executions = if let Some(existing) = existing {
            if self.height()? != Some(height) || existing.hash() != block_hash {
                bail!("validator storage is not at the expected contiguous height");
            }
            None
        } else {
            let expected = self.height()?.map_or(0, |index| {
                index.checked_add(1).expect("block height overflow")
            });
            if height != expected {
                bail!("non-contiguous block {height}; expected {expected}");
            }
            let executed = self
                .system
                .persist_block_without_runtime_cache(block)
                .map_err(|error| anyhow::anyhow!("neo-rs block execution failed: {error}"))?;
            Some(local_receipts(&executed))
        };

        let state_root = self
            .state_handlers
            .get_state_root(height)
            .map(|root| root.root_hash)
            .ok_or_else(|| anyhow::anyhow!("neo-rs did not persist a state root at {height}"))?;
        Ok(ExecutionOutcome {
            block_hash,
            state_root,
            executions,
        })
    }
}

fn local_receipts(executions: &[ApplicationExecuted]) -> Vec<ExecutionReceipt> {
    executions
        .iter()
        .map(|execution| ExecutionReceipt {
            trigger: trigger_name(execution.trigger).to_owned(),
            vm_state: execution
                .vm_state
                .final_name()
                .unwrap_or("NON_FINAL")
                .to_owned(),
            gas_consumed: execution.gas_consumed,
            has_exception: execution
                .exception
                .as_deref()
                .is_some_and(|message| !message.is_empty()),
        })
        .collect()
}

fn trigger_name(trigger: TriggerType) -> &'static str {
    match trigger {
        TriggerType::ON_PERSIST => "OnPersist",
        TriggerType::POST_PERSIST => "PostPersist",
        TriggerType::VERIFICATION => "Verification",
        TriggerType::APPLICATION => "Application",
        TriggerType::SYSTEM => "System",
        _ => "Unknown",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use neo_storage::persistence::providers::memory_store_provider::MemoryStoreProvider;

    #[tokio::test(flavor = "multi_thread")]
    async fn real_genesis_execution_is_persistent_and_idempotent() {
        let settings = ProtocolSettings::default();
        let temp = tempfile::TempDir::new().unwrap();
        let executor = NeoExecutor::open_with_provider(
            settings.clone(),
            Arc::new(MemoryStoreProvider::new()),
            temp.path(),
        )
        .unwrap();
        let genesis = neo_ledger::create_genesis_block(&settings);
        let expected_hash = genesis.clone().hash();
        let first = executor.execute(genesis.clone()).unwrap();
        assert_eq!(first.block_hash, expected_hash);
        assert!(first.executions.is_some());
        assert_eq!(executor.height().unwrap(), Some(0));
        let second = executor.execute(genesis).unwrap();
        assert_eq!(second.state_root, first.state_root);
        assert!(second.executions.is_none());
        executor.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn opt_in_profiling_observes_real_stages_without_changing_state() {
        let settings = ProtocolSettings::default();
        let genesis = neo_ledger::create_genesis_block(&settings);
        let baseline_dir = tempfile::TempDir::new().unwrap();
        let baseline = NeoExecutor::open_with_provider(
            settings.clone(),
            Arc::new(MemoryStoreProvider::new()),
            baseline_dir.path(),
        )
        .unwrap();
        let expected = baseline.execute(genesis.clone()).unwrap();
        assert_eq!(baseline.execution_profile().blocks, 0);
        assert!(!baseline.execution_profile().enabled);
        baseline.shutdown().await.unwrap();

        let profiled_dir = tempfile::TempDir::new().unwrap();
        let profiled = NeoExecutor::open_with_provider(
            settings,
            Arc::new(MemoryStoreProvider::new()),
            profiled_dir.path(),
        )
        .unwrap();
        profiled.set_execution_profiling(true);
        let actual = profiled.execute(genesis).unwrap();
        assert_eq!(actual.state_root, expected.state_root);
        let profile = profiled.execution_profile();
        assert!(profile.enabled);
        assert_eq!(profile.blocks, 1);
        assert_eq!(profile.transaction_executions, 0);
        assert!(!profile.snapshot_open.is_zero());
        assert!(!profile.on_persist.is_zero());
        assert!(!profile.post_persist.is_zero());
        assert!(!profile.participant_precommit.is_zero());
        assert!(!profile.state_change_extraction.is_zero());
        assert!(!profile.mpt.is_zero());
        assert!(!profile.mpt_update.is_zero());
        assert!(!profile.mpt_commit.is_zero());
        assert!(!profile.mpt_root_hash.is_zero());
        assert!(!profile.mpt_hash.is_zero());
        assert!(!profile.mpt_serialization.is_zero());
        assert!(!profile.mpt_hash256.is_zero());
        assert!(!profile.mpt_node_encode.is_zero());
        assert!(!profile.mpt_node_write.is_zero());
        assert!(!profile.store_commit.is_zero());
        profiled.set_execution_profiling(false);
        assert!(!profiled.execution_profile().enabled);
        profiled.shutdown().await.unwrap();
    }

    #[cfg(feature = "rocksdb")]
    #[tokio::test(flavor = "multi_thread")]
    async fn rocksdb_restart_recovers_mpt_and_checkpoint_without_reexecution() {
        let settings = ProtocolSettings::default();
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("validator-store");
        let first = NeoExecutor::open(settings.clone(), &path).unwrap();
        let genesis = neo_ledger::create_genesis_block(&settings);
        let executed = first.execute(genesis.clone()).unwrap();
        first.shutdown().await.unwrap();
        drop(first);

        let reopened = NeoExecutor::open(settings, &path).unwrap();
        let checkpoint = reopened.checkpoint().unwrap().unwrap();
        assert_eq!(checkpoint.height, 0);
        assert_eq!(checkpoint.block_hash, executed.block_hash);
        assert_eq!(checkpoint.state_root, executed.state_root);
        assert_eq!(
            reopened
                .state_handlers
                .current_root_hash()
                .expect("recovered MPT root"),
            executed.state_root
        );
        let replayed = reopened.execute(genesis).unwrap();
        assert!(replayed.executions.is_none());
        assert_eq!(replayed.state_root, executed.state_root);
        reopened.shutdown().await.unwrap();
    }

    #[cfg(feature = "rocksdb")]
    #[tokio::test(flavor = "multi_thread")]
    async fn consecutive_blocks_advance_pointer_before_return_and_after_restart() {
        use neo_primitives::UInt160;
        use neo_protocol::payloads::Header;
        use neo_protocol::witness::Witness;

        let settings = ProtocolSettings::default();
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("validator-store");
        let executor = NeoExecutor::open(settings.clone(), &path).unwrap();
        let mut previous = neo_ledger::create_genesis_block(&settings);
        executor.execute(previous.clone()).unwrap();

        let mut final_outcome = None;
        for index in 1..=3 {
            let mut header = Header::new();
            header.set_index(index);
            header.set_prev_hash(previous.hash());
            header.set_next_consensus(UInt160::zero());
            header.set_timestamp(u64::from(index) * 1_000);
            header.witness = Witness::new();
            let mut block = Block {
                header,
                transactions: Vec::new(),
            };
            let outcome = executor.execute(block.clone()).unwrap();
            assert_eq!(executor.height().unwrap(), Some(index));
            assert_eq!(executor.checkpoint().unwrap().unwrap().height, index);
            assert_eq!(
                executor.checkpoint().unwrap().unwrap().block_hash,
                block.hash()
            );
            previous = block;
            final_outcome = Some(outcome);
        }
        let final_outcome = final_outcome.unwrap();
        executor.shutdown().await.unwrap();
        drop(executor);

        let reopened = NeoExecutor::open(settings, &path).unwrap();
        assert_eq!(reopened.height().unwrap(), Some(3));
        let checkpoint = reopened.checkpoint().unwrap().unwrap();
        assert_eq!(checkpoint.height, 3);
        assert_eq!(checkpoint.block_hash, previous.hash());
        assert_eq!(checkpoint.state_root, final_outcome.state_root);
        assert_eq!(
            reopened.state_handlers.current_root_hash(),
            Some(final_outcome.state_root)
        );
        let replayed = reopened.execute(previous).unwrap();
        assert!(replayed.executions.is_none());
        assert_eq!(replayed.state_root, final_outcome.state_root);
        reopened.shutdown().await.unwrap();
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn empty_block_write_back_matches_every_single_block_result() {
        use neo_primitives::UInt160;
        use neo_protocol::payloads::Header;
        use neo_protocol::witness::Witness;

        let settings = ProtocolSettings::default();
        let baseline_dir = tempfile::TempDir::new().unwrap();
        let batched_dir = tempfile::TempDir::new().unwrap();
        let baseline = NeoExecutor::open_with_provider(
            settings.clone(),
            Arc::new(MemoryStoreProvider::new()),
            baseline_dir.path(),
        )
        .unwrap();
        let batched = NeoExecutor::open_with_provider(
            settings.clone(),
            Arc::new(MemoryStoreProvider::new()),
            batched_dir.path(),
        )
        .unwrap();
        let genesis = neo_ledger::create_genesis_block(&settings);
        assert_eq!(
            baseline.execute(genesis.clone()).unwrap().state_root,
            batched.execute(genesis.clone()).unwrap().state_root
        );

        let block_count = 2 * settings.committee_members_count() as u32 + 2;
        let mut blocks = Vec::with_capacity(block_count as usize);
        let mut previous = genesis;
        for index in 1..=block_count {
            let mut header = Header::new();
            header.set_index(index);
            header.set_prev_hash(previous.hash());
            header.set_next_consensus(UInt160::zero());
            header.set_timestamp(u64::from(index) * 1_000);
            header.witness = Witness::new();
            let block = Block {
                header,
                transactions: Vec::new(),
            };
            blocks.push(block.clone());
            previous = block;
        }

        let expected = blocks
            .iter()
            .cloned()
            .map(|block| baseline.execute(block).unwrap())
            .collect::<Vec<_>>();
        let actual = batched.execute_empty_blocks(blocks).unwrap();
        assert_eq!(actual.len(), expected.len());
        for (height, (expected, actual)) in expected.iter().zip(&actual).enumerate() {
            assert_eq!(actual.block_hash, expected.block_hash, "hash at {height}");
            assert_eq!(actual.state_root, expected.state_root, "root at {height}");
            assert_eq!(
                actual.executions, expected.executions,
                "receipts at {height}"
            );
        }
        assert_eq!(
            batched.native_contract_storage(),
            baseline.native_contract_storage()
        );
        assert_eq!(batched.height().unwrap(), baseline.height().unwrap());
        assert_eq!(
            batched.state_handlers.current_root_hash(),
            baseline.state_handlers.current_root_hash()
        );
        baseline.shutdown().await.unwrap();
        batched.shutdown().await.unwrap();
    }

    #[cfg(feature = "rocksdb")]
    #[tokio::test(flavor = "multi_thread")]
    async fn empty_block_write_back_reopens_every_root_and_is_idempotent() {
        use neo_primitives::UInt160;
        use neo_protocol::payloads::Header;
        use neo_protocol::witness::Witness;

        let settings = ProtocolSettings::default();
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("validator-store");
        let executor = NeoExecutor::open(settings.clone(), &path).unwrap();
        let genesis = neo_ledger::create_genesis_block(&settings);
        executor.execute(genesis.clone()).unwrap();
        let mut previous = genesis;
        let mut blocks = Vec::new();
        for index in 1..=8 {
            let mut header = Header::new();
            header.set_index(index);
            header.set_prev_hash(previous.hash());
            header.set_next_consensus(UInt160::zero());
            header.set_timestamp(u64::from(index) * 1_000);
            header.witness = Witness::new();
            let block = Block {
                header,
                transactions: Vec::new(),
            };
            blocks.push(block.clone());
            previous = block;
        }
        let outcomes = executor.execute_empty_blocks(blocks.clone()).unwrap();
        executor.shutdown().await.unwrap();
        drop(executor);

        let reopened = NeoExecutor::open(settings, &path).unwrap();
        for (block, outcome) in blocks.iter().zip(&outcomes) {
            assert_eq!(
                reopened
                    .state_handlers
                    .get_state_root(block.index())
                    .map(|root| root.root_hash),
                Some(outcome.state_root)
            );
        }
        let checkpoint = reopened.checkpoint().unwrap().unwrap();
        assert_eq!(checkpoint.height, 8);
        assert_eq!(checkpoint.block_hash, outcomes[7].block_hash);
        assert_eq!(checkpoint.state_root, outcomes[7].state_root);
        let replayed = reopened.execute(blocks[7].clone()).unwrap();
        assert!(replayed.executions.is_none());
        assert_eq!(replayed.state_root, outcomes[7].state_root);
        reopened.shutdown().await.unwrap();
    }

    #[cfg(feature = "rocksdb")]
    #[tokio::test(flavor = "multi_thread")]
    async fn empty_block_batch_failure_is_atomic_and_poisons_until_reopen() {
        use neo_primitives::UInt160;
        use neo_protocol::payloads::Header;
        use neo_protocol::witness::Witness;

        struct FailSecondBlock;
        impl neo_system::events::handlers::CommittingHandler for FailSecondBlock {
            fn blockchain_committing_handler(
                &self,
                _system: &dyn std::any::Any,
                _block: &neo_protocol::execution::Block,
                _snapshot: &neo_storage::persistence::DataCache,
                _application_executed_list: &[ApplicationExecuted],
            ) {
            }

            fn try_blockchain_committing_handler(
                &self,
                _system: &dyn std::any::Any,
                block: &neo_protocol::execution::Block,
                _snapshot: &neo_storage::persistence::DataCache,
                _application_executed_list: &[ApplicationExecuted],
            ) -> neo_system::events::handlers::CoreResult<()> {
                if block.index() == 2 {
                    return Err(
                        std::io::Error::other("injected empty-batch precommit failure").into(),
                    );
                }
                Ok(())
            }
        }

        let settings = ProtocolSettings::default();
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("validator-store");
        let executor = NeoExecutor::open(settings.clone(), &path).unwrap();
        let genesis = neo_ledger::create_genesis_block(&settings);
        executor.execute(genesis.clone()).unwrap();
        executor
            .system
            .register_committing_handler(Arc::new(FailSecondBlock))
            .unwrap();
        let mut blocks = Vec::new();
        let mut previous = genesis;
        for index in 1..=2 {
            let mut header = Header::new();
            header.set_index(index);
            header.set_prev_hash(previous.hash());
            header.set_next_consensus(UInt160::zero());
            header.set_timestamp(u64::from(index) * 1_000);
            header.witness = Witness::new();
            let block = Block {
                header,
                transactions: Vec::new(),
            };
            blocks.push(block.clone());
            previous = block;
        }

        let error = executor.execute_empty_blocks(blocks.clone()).unwrap_err();
        assert!(
            error
                .to_string()
                .contains("injected empty-batch precommit failure")
        );
        assert!(
            executor
                .height()
                .unwrap_err()
                .to_string()
                .contains("poisoned")
        );
        assert!(
            executor
                .execute(blocks[0].clone())
                .unwrap_err()
                .to_string()
                .contains("poisoned")
        );
        assert!(
            executor
                .execute_empty_blocks(blocks.clone())
                .unwrap_err()
                .to_string()
                .contains("poisoned")
        );
        executor.shutdown().await.unwrap();
        drop(executor);

        let reopened = NeoExecutor::open(settings, &path).unwrap();
        assert_eq!(reopened.height().unwrap(), Some(0));
        assert!(reopened.state_handlers.get_state_root(1).is_none());
        assert!(reopened.state_handlers.get_state_root(2).is_none());
        for block in blocks {
            reopened.execute(block).unwrap();
        }
        assert_eq!(reopened.height().unwrap(), Some(2));
        reopened.shutdown().await.unwrap();
    }
}
