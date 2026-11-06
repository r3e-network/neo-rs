//! Persistence pipeline helpers for `NeoSystem`.
//!
//! This module keeps the block execution and commit pipeline isolated from the
//! core orchestration logic.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use super::context::{
    TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION,
    TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION, TransactionExecutionExceptionCategory,
    TransactionExecutionObservation, TransactionExecutionStage,
};
use super::converters::convert_payload_block;

// Type aliases for complex closure types
type StorageGetFn = Arc<dyn Fn(&StorageKey) -> Option<StorageItem> + Send + Sync>;
type StorageFindFn =
    Arc<dyn Fn(Option<&StorageKey>, SeekDirection) -> Vec<(StorageKey, StorageItem)> + Send + Sync>;

/// Hard ceiling for diagnostic metadata retained until the canonical commit succeeds.
///
/// The configured evidence queue may be much larger, but persistence must never
/// allocate in proportion to the number of FAULT transactions in a block.
const MAX_PENDING_FAULT_OBSERVATIONS_PER_BLOCK: usize = 64;

struct PendingFaultObservation {
    transaction_hash: UInt256,
    exception_category: TransactionExecutionExceptionCategory,
    gas_consumed: i64,
}

struct FaultObservationBuffer {
    observations: Vec<PendingFaultObservation>,
    limit: usize,
    dropped: usize,
}

impl FaultObservationBuffer {
    fn for_available_capacity(available_capacity: usize) -> Self {
        let limit = available_capacity.min(MAX_PENDING_FAULT_OBSERVATIONS_PER_BLOCK);
        Self {
            observations: Vec::with_capacity(limit),
            limit,
            dropped: 0,
        }
    }

    fn push(&mut self, observation: PendingFaultObservation) {
        if self.observations.len() < self.limit {
            self.observations.push(observation);
        } else {
            self.dropped = self.dropped.saturating_add(1);
        }
    }
}

use super::NeoSystem;
use crate::events::PluginEvent;
use crate::network::p2p::payloads::block::Block;
use neo_core::error::{CoreError, CoreResult};
use neo_core::ledger::block::Block as LedgerBlock;
use neo_core::ledger::blockchain_application_executed::ApplicationExecuted;
use neo_core::persistence::StoreTransaction;
use neo_core::persistence::data_cache::{
    DataCache, DataCacheConfig, StorageWatchPhase, clear_storage_watch_context,
    set_storage_watch_context,
};
use neo_core::persistence::seek_direction::SeekDirection;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::application_engine::TEST_MODE_GAS;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::native::ledger_contract::{
    LedgerTransactionStates, PersistedTransactionState,
};
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::smart_contract::{StorageItem, StorageKey};
use neo_primitives::UInt256;
use neo_vm::StackItem;
use neo_vm::VmState as VMState;
use tracing::{debug, info, warn};

#[derive(Default)]
struct PersistPerfStats {
    blocks: AtomicU64,
    txs: AtomicU64,
    block_total_ns: AtomicU64,
    on_persist_ns: AtomicU64,
    on_persist_engine_ns: AtomicU64,
    on_persist_native_ns: AtomicU64,
    tx_prepare_ns: AtomicU64,
    tx_execute_ns: AtomicU64,
    tx_merge_ns: AtomicU64,
    post_persist_ns: AtomicU64,
    post_persist_engine_ns: AtomicU64,
    post_persist_native_ns: AtomicU64,
    apply_tracked_ns: AtomicU64,
    committing_ns: AtomicU64,
    commit_ns: AtomicU64,
    committed_ns: AtomicU64,
}

fn persist_perf_stats() -> &'static PersistPerfStats {
    static STATS: OnceLock<PersistPerfStats> = OnceLock::new();
    STATS.get_or_init(PersistPerfStats::default)
}

fn persist_perf_enabled() -> bool {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    *ENABLED.get_or_init(|| {
        std::env::var("NEO_PERSIST_PROFILE")
            .ok()
            .map(|raw| {
                let normalized = raw.trim().to_ascii_lowercase();
                matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
            })
            .unwrap_or(false)
    })
}

fn fault_trace_block_filter() -> Option<u32> {
    static FILTER: OnceLock<Option<u32>> = OnceLock::new();
    *FILTER.get_or_init(|| {
        std::env::var("NEO_TRACE_FAULT_BLOCK").ok().and_then(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            match trimmed.parse::<u32>() {
                Ok(value) => Some(value),
                Err(err) => {
                    warn!(
                        target: "neo",
                        raw = trimmed,
                        error = %err,
                        "invalid NEO_TRACE_FAULT_BLOCK; ignoring filter"
                    );
                    None
                }
            }
        })
    })
}

fn fault_trace_tx_filter() -> Option<&'static UInt256> {
    static FILTER: OnceLock<Option<UInt256>> = OnceLock::new();
    FILTER
        .get_or_init(|| {
            std::env::var("NEO_TRACE_FAULT_TX").ok().and_then(|raw| {
                let trimmed = raw.trim();
                if trimmed.is_empty() {
                    return None;
                }
                let normalized = trimmed
                    .strip_prefix("0x")
                    .or_else(|| trimmed.strip_prefix("0X"))
                    .unwrap_or(trimmed);
                match UInt256::parse(normalized) {
                    Ok(value) => Some(value),
                    Err(err) => {
                        warn!(
                            target: "neo",
                            raw = trimmed,
                            error = %err,
                            "invalid NEO_TRACE_FAULT_TX; ignoring filter"
                        );
                        None
                    }
                }
            })
        })
        .as_ref()
}

fn should_trace_fault(block_index: u32, _tx_hash: &UInt256) -> bool {
    let block_filter = fault_trace_block_filter();
    let tx_filter = fault_trace_tx_filter();
    match (block_filter, tx_filter) {
        (None, None) => false,
        (Some(expected_block), None) => block_index == expected_block,
        (None, Some(expected_tx)) => _tx_hash == expected_tx,
        (Some(expected_block), Some(expected_tx)) => {
            block_index == expected_block && _tx_hash == expected_tx
        }
    }
}

/// Mirrors the engine's private `NEO_TRACE_BLOCK` range check.
///
/// `ApplicationEngine::should_trace_block` is `pub(crate)` inside `neo-smart-contract`, so the
/// persistence pipeline keeps a local copy of the same environment-driven rule. Accepts either
/// a single index (`NEO_TRACE_BLOCK=42`) or an inclusive range (`NEO_TRACE_BLOCK=40-50`).
fn should_trace_block(block_index: u32) -> bool {
    static RANGE: OnceLock<Option<(u32, u32)>> = OnceLock::new();
    let range = RANGE.get_or_init(|| {
        std::env::var("NEO_TRACE_BLOCK").ok().and_then(|raw| {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                return None;
            }
            if let Some((start, end)) = trimmed.split_once('-') {
                let start = start.trim().parse::<u32>().ok()?;
                let end = end.trim().parse::<u32>().ok()?;
                Some((start, end))
            } else {
                let block = trimmed.parse::<u32>().ok()?;
                Some((block, block))
            }
        })
    });
    range
        .map(|(start, end)| block_index >= start && block_index <= end)
        .unwrap_or(false)
}

/// Records a successful native OnPersist/PostPersist run on the engine.
///
/// Native persist does not enter the VM loop, so the engine stays in `BREAK`; C#
/// `ApplicationEngine.Execute` reports a successful native persist as `HALT`. The canonical
/// helper in `neo-smart-contract` is gated behind a `runtime` feature that crate does not
/// define, so the transition is applied here through the public execute path.
fn record_native_persist_success(engine: &mut ApplicationEngine) {
    if engine.state() == VMState::BREAK {
        engine.execute_allow_fault();
    }
}

fn saturating_ns(duration: std::time::Duration) -> u64 {
    let nanos = duration.as_nanos();
    nanos.min(u128::from(u64::MAX)) as u64
}

fn observation_time_unix_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

struct StorageWatchContextGuard;

impl Drop for StorageWatchContextGuard {
    fn drop(&mut self) {
        clear_storage_watch_context();
    }
}

impl NeoSystem {
    /// Persists a block through the minimal smart-contract pipeline, returning
    /// the list of execution summaries produced during processing.
    pub fn persist_block(&self, block: Block) -> CoreResult<Vec<ApplicationExecuted>> {
        self.persist_block_internal(block, true)
    }

    /// Persists a block while skipping insertion into the in-memory block/header
    /// cache. This is intended for offline import paths where runtime query
    /// caches are not needed.
    pub fn persist_block_without_runtime_cache(
        &self,
        block: Block,
    ) -> CoreResult<Vec<ApplicationExecuted>> {
        self.persist_block_internal(block, false)
    }

    fn persist_block_internal(
        &self,
        block: Block,
        update_runtime_cache: bool,
    ) -> CoreResult<Vec<ApplicationExecuted>> {
        let is_fast_sync = self.context().is_fast_sync_mode();
        let has_fast_sync_handlers = is_fast_sync && {
            let handlers = self.context().committing_handlers().read().clone();
            handlers.iter().any(|h| h.run_during_fast_sync())
        };
        let emit_detailed_execution = !is_fast_sync || has_fast_sync_handlers;
        let perf_enabled = persist_perf_enabled();
        let block_started = if perf_enabled {
            Some(Instant::now())
        } else {
            None
        };

        let ledger_block = convert_payload_block(&block);
        let block_hash = ledger_block.hash();
        let mut fault_observations = FaultObservationBuffer::for_available_capacity(
            self.context()
                .transaction_execution_sink_available_capacity(),
        );
        let _watch_context_guard = StorageWatchContextGuard;
        let tx_count = ledger_block.transactions.len() as u64;
        // The engine persists the canonical network payload block, while the ledger handlers
        // below operate on the storage-oriented `LedgerBlock` projection.
        let persisting_block = Arc::new(block.clone());
        let base_cache_config = DataCacheConfig {
            // Keep block-local hot keys in memory so subsequent transactions in
            // the same block avoid repeated RocksDB lookups.
            track_reads_in_write_cache: true,
            enable_read_cache: false,
            enable_prefetching: false,
            ..Default::default()
        };
        let tx_cache_config = DataCacheConfig {
            // Transaction overlays are short-lived and discarded on FAULT, so
            // avoid duplicating read entries into each tx-local cache.
            track_reads_in_write_cache: false,
            enable_read_cache: false,
            enable_prefetching: false,
            ..Default::default()
        };
        let tx =
            StoreTransaction::from_snapshot_with_config(self.store().snapshot(), base_cache_config);
        let base_snapshot = Arc::new(tx.cache().data_cache().clone());
        if ledger_block.transactions.len() >= 2 {
            use rayon::prelude::*;
            ledger_block
                .transactions
                .par_iter()
                .try_for_each(|tx| tx.try_hash().map(|_| ()))?;
        }

        let on_persist_started = if perf_enabled {
            Some(Instant::now())
        } else {
            None
        };
        let on_persist_engine_started = if perf_enabled {
            Some(Instant::now())
        } else {
            None
        };
        let mut on_persist_engine = ApplicationEngine::new_with_shared_block(
            TriggerType::OnPersist,
            None,
            Arc::clone(&base_snapshot),
            Some(Arc::clone(&persisting_block)),
            self.settings().clone(),
            TEST_MODE_GAS,
            None,
            neo_core::smart_contract::native::composition::default_native_services(),
        )?;
        if let Some(started) = on_persist_engine_started {
            persist_perf_stats()
                .on_persist_engine_ns
                .fetch_add(saturating_ns(started.elapsed()), Ordering::Relaxed);
        }

        let on_persist_native_started = if perf_enabled {
            Some(Instant::now())
        } else {
            None
        };
        set_storage_watch_context(ledger_block.index(), StorageWatchPhase::OnPersist, None);
        on_persist_engine.native_on_persist()?;
        record_native_persist_success(&mut on_persist_engine);
        if let Some(started) = on_persist_native_started {
            persist_perf_stats()
                .on_persist_native_ns
                .fetch_add(saturating_ns(started.elapsed()), Ordering::Relaxed);
        }
        if let Some(started) = on_persist_started {
            persist_perf_stats()
                .on_persist_ns
                .fetch_add(saturating_ns(started.elapsed()), Ordering::Relaxed);
        }
        let mut executed = if emit_detailed_execution {
            let on_persist_exec = ApplicationExecuted::new(&mut on_persist_engine);
            self.actor_system()
                .event_stream()
                .publish(on_persist_exec.clone());
            let mut records = Vec::with_capacity(ledger_block.transactions.len() + 2);
            records.push(on_persist_exec);
            records
        } else {
            Vec::new()
        };
        let native_hashes: std::collections::HashSet<_> = on_persist_engine
            .native_contracts()
            .into_iter()
            .map(|contract| contract.hash())
            .collect();
        let mut seeded_contracts = on_persist_engine.contracts_shared_snapshot();
        seeded_contracts.retain(|hash, _| native_hashes.contains(hash));
        let seeded_native_cache = on_persist_engine.native_contract_cache_handle();

        let mut tx_states = on_persist_engine
            .take_state::<LedgerTransactionStates>()
            .unwrap_or_else(|| {
                LedgerTransactionStates::new(Vec::<PersistedTransactionState>::new())
            });

        if !ledger_block.transactions.is_empty() {
            let tx_store_get: StorageGetFn = {
                let base = Arc::clone(&base_snapshot);
                Arc::new(move |key: &StorageKey| base.get(key))
            };
            let tx_store_find: StorageFindFn = {
                let base = Arc::clone(&base_snapshot);
                Arc::new(
                    move |prefix: Option<&StorageKey>, direction: SeekDirection| {
                        base.find(prefix, direction).collect::<Vec<_>>()
                    },
                )
            };
            let tx_snapshot = Arc::new(DataCache::new_with_config(
                false,
                Some(tx_store_get),
                Some(tx_store_find),
                tx_cache_config,
            ));
            for tx in &ledger_block.transactions {
                let tx_hash = tx.try_hash()?;
                set_storage_watch_context(
                    ledger_block.index(),
                    StorageWatchPhase::Application,
                    Some(tx_hash),
                );
                let tx_prepare_started = if perf_enabled {
                    Some(Instant::now())
                } else {
                    None
                };
                let container: Arc<dyn neo_core::Verifiable> = Arc::new(tx.clone());
                let mut tx_engine = ApplicationEngine::new_with_preloaded_native_shared(
                    TriggerType::Application,
                    Some(container),
                    Arc::clone(&tx_snapshot),
                    Some(Arc::clone(&persisting_block)),
                    self.settings().clone(),
                    tx.system_fee(),
                    seeded_contracts.clone(),
                    Arc::clone(&seeded_native_cache),
                    None,
                    neo_core::smart_contract::native::composition::default_native_services(),
                )?;

                tx_engine.set_state(tx_states);
                tx_engine.load_script(tx.script().to_vec(), CallFlags::ALL, None)?;
                if let Some(started) = tx_prepare_started {
                    persist_perf_stats()
                        .tx_prepare_ns
                        .fetch_add(saturating_ns(started.elapsed()), Ordering::Relaxed);
                }

                let tx_exec_started = if perf_enabled {
                    Some(Instant::now())
                } else {
                    None
                };
                let vm_state = tx_engine.execute_allow_fault();
                // Diagnostic: log execution details for traced blocks (NEO_TRACE_BLOCK env var)
                if should_trace_block(ledger_block.index()) {
                    let gas_consumed = tx_engine.gas_consumed();
                    let notif_count = tx_engine.notifications().len();
                    let exception = tx_engine.fault_exception().map(|s| s.to_string());
                    warn!(
                        target: "neo",
                        block_index = ledger_block.index(),
                        %tx_hash,
                        ?vm_state,
                        gas_consumed,
                        notif_count,
                        exception = ?exception,
                        "TRACE: tx execution result"
                    );
                    for (idx, notif) in tx_engine.notifications().iter().enumerate() {
                        let state_items: Vec<String> = notif
                            .state
                            .iter()
                            .map(|item| match item {
                                StackItem::Null => "null".to_string(),
                                StackItem::Boolean(b) => format!("bool:{}", b),
                                StackItem::Integer(n) => format!("int:{}", n),
                                StackItem::ByteString(b) => {
                                    format!("bytes:0x{}", hex::encode(b))
                                }
                                _ => format!("{:?}", item.stack_item_type()),
                            })
                            .collect();
                        warn!(
                            target: "neo",
                            block_index = ledger_block.index(),
                            %tx_hash,
                            notif_idx = idx,
                            contract = %notif.script_hash,
                            event = %notif.event_name,
                            state = ?state_items,
                            "TRACE: notification"
                        );
                    }
                }
                if let Some(started) = tx_exec_started {
                    persist_perf_stats()
                        .tx_execute_ns
                        .fetch_add(saturating_ns(started.elapsed()), Ordering::Relaxed);
                }
                if emit_detailed_execution {
                    let executed_tx = ApplicationExecuted::new(&mut tx_engine);
                    self.actor_system()
                        .event_stream()
                        .publish(executed_tx.clone());
                    executed.push(executed_tx);
                } else {
                    // Keep ledger transaction VM states accurate without building
                    // full `ApplicationExecuted` payloads during fast-sync/import.
                    let _ = tx_engine.record_transaction_vm_state(&tx_hash, vm_state);
                }
                tx_states = tx_engine
                    .take_state::<LedgerTransactionStates>()
                    .unwrap_or_else(|| {
                        LedgerTransactionStates::new(Vec::<PersistedTransactionState>::new())
                    });

                let tx_merge_started = if perf_enabled {
                    Some(Instant::now())
                } else {
                    None
                };
                match vm_state {
                    VMState::HALT => {
                        base_snapshot.merge_from_child(&tx_snapshot);
                    }
                    VMState::FAULT => {
                        tx_snapshot.reset();
                        let exception_category = if tx_engine.fault_exception().is_some() {
                            TransactionExecutionExceptionCategory::CaughtVmException
                        } else {
                            TransactionExecutionExceptionCategory::VmFault
                        };
                        fault_observations.push(PendingFaultObservation {
                            transaction_hash: tx_hash,
                            exception_category,
                            gas_consumed: tx_engine.fee_consumed(),
                        });
                        if should_trace_fault(ledger_block.index(), &tx_hash) {
                            warn!(
                                target: "neo",
                                %tx_hash,
                                block_index = ledger_block.index(),
                                exception = tx_engine.fault_exception().unwrap_or("<none>"),
                                "transaction execution faulted; skipping storage merge"
                            );
                        } else {
                            debug!(
                                target: "neo",
                                %tx_hash,
                                block_index = ledger_block.index(),
                                exception = ?tx_engine.fault_exception(),
                                "transaction execution faulted; skipping storage merge"
                            );
                        }
                    }
                    other => {
                        return Err(CoreError::system(format!(
                            "unexpected transaction VM state {:?} for hash {}",
                            other, tx_hash
                        )));
                    }
                }
                if let Some(started) = tx_merge_started {
                    persist_perf_stats()
                        .tx_merge_ns
                        .fetch_add(saturating_ns(started.elapsed()), Ordering::Relaxed);
                }
            }
        }

        let post_persist_started = if perf_enabled {
            Some(Instant::now())
        } else {
            None
        };
        let post_persist_engine_started = if perf_enabled {
            Some(Instant::now())
        } else {
            None
        };
        let mut post_persist_engine = ApplicationEngine::new_with_preloaded_native_shared(
            TriggerType::PostPersist,
            None,
            Arc::clone(&base_snapshot),
            Some(Arc::clone(&persisting_block)),
            self.settings().clone(),
            TEST_MODE_GAS,
            seeded_contracts,
            seeded_native_cache,
            None,
            neo_core::smart_contract::native::composition::default_native_services(),
        )?;
        if let Some(started) = post_persist_engine_started {
            persist_perf_stats()
                .post_persist_engine_ns
                .fetch_add(saturating_ns(started.elapsed()), Ordering::Relaxed);
        }
        let post_persist_native_started = if perf_enabled {
            Some(Instant::now())
        } else {
            None
        };
        post_persist_engine.set_state(tx_states);
        set_storage_watch_context(ledger_block.index(), StorageWatchPhase::PostPersist, None);
        post_persist_engine.native_post_persist()?;
        record_native_persist_success(&mut post_persist_engine);
        if let Some(started) = post_persist_native_started {
            persist_perf_stats()
                .post_persist_native_ns
                .fetch_add(saturating_ns(started.elapsed()), Ordering::Relaxed);
        }
        if let Some(started) = post_persist_started {
            persist_perf_stats()
                .post_persist_ns
                .fetch_add(saturating_ns(started.elapsed()), Ordering::Relaxed);
        }
        if emit_detailed_execution {
            let post_persist_exec = ApplicationExecuted::new(&mut post_persist_engine);
            self.actor_system()
                .event_stream()
                .publish(post_persist_exec.clone());
            executed.push(post_persist_exec);
        }

        // State root handlers must always run to keep the state trie up to date.
        // Other expensive handlers (application logs, etc.) are skipped during fast sync.
        let committing_started = if perf_enabled {
            Some(Instant::now())
        } else {
            None
        };
        if let Err(error) = self.invoke_committing(&ledger_block, base_snapshot.as_ref(), &executed)
        {
            if let Ok(Some(state_store)) = self.state_store() {
                state_store.discard_staged_local_state_root();
            }
            return Err(error);
        }
        if let Some(started) = committing_started {
            persist_perf_stats()
                .committing_ns
                .fetch_add(saturating_ns(started.elapsed()), Ordering::Relaxed);
        }

        let commit_started = if perf_enabled {
            Some(Instant::now())
        } else {
            None
        };
        if let Err(err) = tx.commit() {
            if let Ok(Some(state_store)) = self.state_store() {
                state_store.discard_staged_local_state_root();
            }
            return Err(CoreError::system(format!(
                "failed to commit store cache for block {}: {err}",
                ledger_block.index()
            )));
        }
        if let Some(started) = commit_started {
            persist_perf_stats()
                .commit_ns
                .fetch_add(saturating_ns(started.elapsed()), Ordering::Relaxed);
        }

        if update_runtime_cache {
            // Update in-memory caches with the payload block so networking queries can respond immediately.
            if let Err(err) = self.context().record_block(block.clone()) {
                if let Ok(Some(state_store)) = self.state_store() {
                    state_store.discard_staged_local_state_root();
                }
                return Err(CoreError::system(format!(
                    "failed to update runtime block cache for block {}: {err}",
                    ledger_block.index()
                )));
            }
        } else {
            // Keep tip height moving for helper call sites during offline imports.
            self.context()
                .ledger_handle()
                .record_tip(ledger_block.index());
        }

        // State root committed handler must always run.
        let committed_started = if perf_enabled {
            Some(Instant::now())
        } else {
            None
        };
        self.invoke_committed(&ledger_block);
        if let Some(started) = committed_started {
            persist_perf_stats()
                .committed_ns
                .fetch_add(saturating_ns(started.elapsed()), Ordering::Relaxed);
        }

        // Observations are exported only after a successful canonical commit. The
        // wall clock and observer outcome are diagnostic-only and never feed back
        // into execution, persistence, validity, or replay decisions.
        let mut dropped_fault_observations = fault_observations.dropped;
        for observation in fault_observations.observations {
            let emitted = self.context().try_emit_transaction_execution_observation(
                TransactionExecutionObservation {
                    transaction_hash: observation.transaction_hash,
                    block_height: ledger_block.index(),
                    block_hash,
                    execution_stage: TransactionExecutionStage::Application,
                    vm_state: VMState::FAULT,
                    exception_category: observation.exception_category,
                    gas_consumed: observation.gas_consumed,
                    schema_version: TRANSACTION_EXECUTION_OBSERVATION_SCHEMA_VERSION,
                    rule_version: TRANSACTION_EXECUTION_OBSERVATION_RULE_VERSION,
                    observed_at_unix_ms: observation_time_unix_ms(),
                },
            );
            dropped_fault_observations =
                dropped_fault_observations.saturating_add(usize::from(!emitted));
        }
        self.context()
            .record_dropped_transaction_execution_observations(dropped_fault_observations);

        // Skip expensive plugin events during fast sync
        if emit_detailed_execution {
            // Notify plugins that a block has been persisted, matching the C# event ordering.
            let block_hash = ledger_block.hash().to_string();
            let block_height = ledger_block.index();
            self.context()
                .broadcast_plugin_event(PluginEvent::BlockReceived {
                    block_hash,
                    block_height,
                });
        }

        if let Some(started) = block_started {
            let stats = persist_perf_stats();
            let total_ns = saturating_ns(started.elapsed());
            stats.block_total_ns.fetch_add(total_ns, Ordering::Relaxed);
            stats.txs.fetch_add(tx_count, Ordering::Relaxed);
            let blocks = stats.blocks.fetch_add(1, Ordering::Relaxed) + 1;

            if blocks.is_multiple_of(1000) {
                let blocks_f = blocks as f64;
                let txs = stats.txs.load(Ordering::Relaxed);
                let txs_f = txs.max(1) as f64;

                let total_ms_per_block =
                    stats.block_total_ns.load(Ordering::Relaxed) as f64 / blocks_f / 1_000_000.0;
                let on_persist_ms =
                    stats.on_persist_ns.load(Ordering::Relaxed) as f64 / blocks_f / 1_000_000.0;
                let on_persist_engine_ms = stats.on_persist_engine_ns.load(Ordering::Relaxed)
                    as f64
                    / blocks_f
                    / 1_000_000.0;
                let on_persist_native_ms = stats.on_persist_native_ns.load(Ordering::Relaxed)
                    as f64
                    / blocks_f
                    / 1_000_000.0;
                let tx_prepare_us =
                    stats.tx_prepare_ns.load(Ordering::Relaxed) as f64 / txs_f / 1_000.0;
                let tx_execute_us =
                    stats.tx_execute_ns.load(Ordering::Relaxed) as f64 / txs_f / 1_000.0;
                let tx_merge_us =
                    stats.tx_merge_ns.load(Ordering::Relaxed) as f64 / txs_f / 1_000.0;
                let post_persist_ms =
                    stats.post_persist_ns.load(Ordering::Relaxed) as f64 / blocks_f / 1_000_000.0;
                let post_persist_engine_ms = stats.post_persist_engine_ns.load(Ordering::Relaxed)
                    as f64
                    / blocks_f
                    / 1_000_000.0;
                let post_persist_native_ms = stats.post_persist_native_ns.load(Ordering::Relaxed)
                    as f64
                    / blocks_f
                    / 1_000_000.0;
                let apply_tracked_ms =
                    stats.apply_tracked_ns.load(Ordering::Relaxed) as f64 / blocks_f / 1_000_000.0;
                let committing_ms =
                    stats.committing_ns.load(Ordering::Relaxed) as f64 / blocks_f / 1_000_000.0;
                let commit_ms =
                    stats.commit_ns.load(Ordering::Relaxed) as f64 / blocks_f / 1_000_000.0;
                let committed_ms =
                    stats.committed_ns.load(Ordering::Relaxed) as f64 / blocks_f / 1_000_000.0;

                info!(
                    target: "neo",
                    blocks,
                    txs,
                    avg_block_ms = total_ms_per_block,
                    avg_on_persist_ms = on_persist_ms,
                    avg_on_persist_engine_ms = on_persist_engine_ms,
                    avg_on_persist_native_ms = on_persist_native_ms,
                    avg_tx_prepare_us = tx_prepare_us,
                    avg_tx_execute_us = tx_execute_us,
                    avg_tx_merge_us = tx_merge_us,
                    avg_post_persist_ms = post_persist_ms,
                    avg_post_persist_engine_ms = post_persist_engine_ms,
                    avg_post_persist_native_ms = post_persist_native_ms,
                    avg_apply_tracked_ms = apply_tracked_ms,
                    avg_committing_ms = committing_ms,
                    avg_commit_ms = commit_ms,
                    avg_committed_ms = committed_ms,
                    "persist pipeline profile"
                );
            }
        }

        Ok(executed)
    }

    fn invoke_committing(
        &self,
        block: &LedgerBlock,
        snapshot: &DataCache,
        application_executed: &[ApplicationExecuted],
    ) -> CoreResult<()> {
        let is_fast_sync = self.context().is_fast_sync_mode();
        let handlers = { self.context().committing_handlers().read().clone() };
        for handler in handlers {
            if is_fast_sync && !handler.run_during_fast_sync() {
                continue;
            }
            handler.try_blockchain_committing_handler(
                self,
                block,
                snapshot,
                application_executed,
            )?;
        }
        Ok(())
    }

    fn invoke_committed(&self, block: &LedgerBlock) {
        let handlers = { self.context().committed_handlers().read().clone() };
        for handler in handlers {
            handler.blockchain_committed_handler(self, block);
        }
    }
}
