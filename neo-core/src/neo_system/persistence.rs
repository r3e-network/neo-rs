//! Persistence pipeline helpers for `NeoSystem`.
//!
//! This module keeps the block execution and commit pipeline isolated from the
//! core orchestration logic.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use super::converters::convert_payload_block;
use super::NeoSystem;
use crate::error::{CoreError, CoreResult};
use crate::events::PluginEvent;
use crate::ledger::block::Block as LedgerBlock;
use crate::ledger::blockchain_application_executed::ApplicationExecuted;
use crate::network::p2p::payloads::block::Block;
use crate::persistence::data_cache::{
    clear_storage_watch_context, set_storage_watch_context, DataCache, DataCacheConfig,
    StorageWatchPhase,
};
use crate::persistence::seek_direction::SeekDirection;
use crate::persistence::StoreTransaction;
use crate::smart_contract::application_engine::ApplicationEngine;
use crate::smart_contract::application_engine::TEST_MODE_GAS;
use crate::smart_contract::call_flags::CallFlags;
use crate::smart_contract::native::ledger_contract::{
    LedgerTransactionStates, PersistedTransactionState,
};
use crate::smart_contract::trigger_type::TriggerType;
use crate::smart_contract::{StorageItem, StorageKey};
use crate::UInt256;
use neo_vm::vm_state::VMState;
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
    commit_ns: AtomicU64,
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
        std::env::var("NEO_TRACE_FAULT_BLOCK")
            .ok()
            .and_then(|raw| {
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
            std::env::var("NEO_TRACE_FAULT_TX")
                .ok()
                .and_then(|raw| {
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

fn should_trace_fault(block_index: u32, tx_hash: &UInt256) -> bool {
    let block_filter = fault_trace_block_filter();
    let tx_filter = fault_trace_tx_filter();
    match (block_filter, tx_filter) {
        (None, None) => false,
        (Some(expected_block), None) => block_index == expected_block,
        (None, Some(expected_tx)) => tx_hash == expected_tx,
        (Some(expected_block), Some(expected_tx)) => {
            block_index == expected_block && tx_hash == expected_tx
        }
    }
}

fn saturating_ns(duration: std::time::Duration) -> u64 {
    let nanos = duration.as_nanos();
    nanos.min(u128::from(u64::MAX)) as u64
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
        let emit_detailed_execution = !self.context().is_fast_sync_mode();
        let perf_enabled = persist_perf_enabled();
        let block_started = if perf_enabled {
            Some(Instant::now())
        } else {
            None
        };

        let ledger_block = convert_payload_block(&block);
        let _watch_context_guard = StorageWatchContextGuard;
        let tx_count = ledger_block.transactions.len() as u64;
        let persisting_block = Arc::new(ledger_block.clone());
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
        let mut tx = StoreTransaction::from_snapshot_with_config(
            self.store().get_snapshot(),
            base_cache_config,
        );
        let base_snapshot = Arc::new(tx.cache().data_cache().clone());
        let tx_store_get: Arc<dyn Fn(&StorageKey) -> Option<StorageItem> + Send + Sync> = {
            let base = Arc::clone(&base_snapshot);
            Arc::new(move |key: &StorageKey| base.get(key))
        };
        let tx_store_find: Arc<
            dyn Fn(Option<&StorageKey>, SeekDirection) -> Vec<(StorageKey, StorageItem)>
                + Send
                + Sync,
        > = {
            let base = Arc::clone(&base_snapshot);
            Arc::new(
                move |prefix: Option<&StorageKey>, direction: SeekDirection| {
                    base.find(prefix, direction).collect::<Vec<_>>()
                },
            )
        };

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
        let mut seeded_contracts = on_persist_engine.contracts_snapshot();
        seeded_contracts.retain(|hash, _| native_hashes.contains(hash));
        let seeded_native_cache = on_persist_engine.native_contract_cache_handle();

        let mut tx_states = on_persist_engine
            .take_state::<LedgerTransactionStates>()
            .unwrap_or_else(|| {
                LedgerTransactionStates::new(Vec::<PersistedTransactionState>::new())
            });

        for tx in &ledger_block.transactions {
            let tx_hash = tx.hash();
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
            let tx_snapshot = Arc::new(DataCache::new_with_config(
                false,
                Some(Arc::clone(&tx_store_get)),
                Some(Arc::clone(&tx_store_find)),
                tx_cache_config,
            ));
            let container: Arc<dyn crate::IVerifiable> = Arc::new(tx.clone());
            let mut tx_engine = ApplicationEngine::new_with_preloaded_native(
                TriggerType::Application,
                Some(container),
                Arc::clone(&tx_snapshot),
                Some(Arc::clone(&persisting_block)),
                self.settings().clone(),
                tx.system_fee(),
                seeded_contracts.clone(),
                Arc::clone(&seeded_native_cache),
                None,
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
                    let tracked = tx_snapshot.tracked_items();
                    base_snapshot.merge_tracked_items(&tracked);
                }
                VMState::FAULT => {
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
        let mut post_persist_engine = ApplicationEngine::new_with_preloaded_native(
            TriggerType::PostPersist,
            None,
            Arc::clone(&base_snapshot),
            Some(Arc::clone(&persisting_block)),
            self.settings().clone(),
            TEST_MODE_GAS,
            seeded_contracts,
            seeded_native_cache,
            None,
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

        // Skip expensive handler calls during fast sync
        if emit_detailed_execution {
            self.invoke_committing(&ledger_block, base_snapshot.as_ref(), &executed);
        }

        let apply_tracked_started = if perf_enabled {
            Some(Instant::now())
        } else {
            None
        };
        crate::persistence::transaction::apply_tracked_items(
            tx.cache_mut(),
            base_snapshot.tracked_items(),
        );
        if let Some(started) = apply_tracked_started {
            persist_perf_stats()
                .apply_tracked_ns
                .fetch_add(saturating_ns(started.elapsed()), Ordering::Relaxed);
        }

        let commit_started = if perf_enabled {
            Some(Instant::now())
        } else {
            None
        };
        tx.commit().map_err(|err| {
            CoreError::system(format!(
                "failed to commit store cache for block {}: {err}",
                ledger_block.index()
            ))
        })?;
        if let Some(started) = commit_started {
            persist_perf_stats()
                .commit_ns
                .fetch_add(saturating_ns(started.elapsed()), Ordering::Relaxed);
        }

        if update_runtime_cache {
            // Update in-memory caches with the payload block so networking queries can respond immediately.
            self.context().record_block(block.clone());
        } else {
            // Keep tip height moving for helper call sites during offline imports.
            self.context()
                .ledger_handle()
                .record_tip(ledger_block.index());
        }

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

            self.invoke_committed(&ledger_block);
        }

        if let Some(started) = block_started {
            let stats = persist_perf_stats();
            let total_ns = saturating_ns(started.elapsed());
            stats.block_total_ns.fetch_add(total_ns, Ordering::Relaxed);
            stats.txs.fetch_add(tx_count, Ordering::Relaxed);
            let blocks = stats.blocks.fetch_add(1, Ordering::Relaxed) + 1;

            if blocks % 1000 == 0 {
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
                let commit_ms =
                    stats.commit_ns.load(Ordering::Relaxed) as f64 / blocks_f / 1_000_000.0;

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
                    avg_commit_ms = commit_ms,
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
    ) {
        let handlers = { self.context().committing_handlers().read().clone() };
        for handler in handlers {
            handler.blockchain_committing_handler(self, block, snapshot, application_executed);
        }
    }

    fn invoke_committed(&self, block: &LedgerBlock) {
        let handlers = { self.context().committed_handlers().read().clone() };
        for handler in handlers {
            handler.blockchain_committed_handler(self, block);
        }
    }
}
