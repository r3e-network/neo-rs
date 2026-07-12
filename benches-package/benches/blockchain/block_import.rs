#![allow(missing_docs)] // benchmark/integration-test harness: not public API

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use criterion::{BatchSize, Bencher, BenchmarkId, Criterion, criterion_group, criterion_main};
use neo_blockchain::{
    BlockPersistContext, BlockchainHandle, ImportBlocksStats, SyncBatchCommitPolicy,
};
use neo_config::ProtocolSettings;
use neo_manifest::CallFlags;
use neo_native_contracts::{GasToken, StandardNativeProvider};
use neo_payloads::{Block, Signer, Transaction, Witness, header::Header};
use neo_primitives::{UInt160, UInt256, WitnessScope};
use neo_state_service::{
    StateRootApplyMetrics, StateStore, commit_handlers::StateServiceCommitHandlers,
};
use neo_storage::{
    CacheRead, DataCache,
    mdbx::MdbxStore,
    persistence::{Store, TransactionalStore, providers::MemoryStore},
};
use neo_system::{BlockCommitHooks, NodeCoreBuilder};
use neo_vm::script_builder::ScriptBuilder;
use num_bigint::BigInt;

#[path = "../support/mod.rs"]
mod support;

use support::make_mdbx_store;

const BLOCKS_PER_BATCH: usize = 32;
const DEFAULT_MPT_APPLY_BATCH_BLOCKS: usize = 8;
const TRANSFER_AMOUNT: i64 = 1;
const SYSTEM_FEE: i64 = 1_0000_0000;
const INITIAL_GAS_BALANCE: i64 = 1_000_000_000_000_000_000;

struct StateServiceHooks<S: Store> {
    handlers: StateServiceCommitHandlers<S>,
}

impl<S: Store> StateServiceHooks<S> {
    fn new(state_store: Arc<StateStore<S>>, max_apply_batch_blocks: usize) -> Self {
        Self {
            handlers: StateServiceCommitHandlers::new_async_with_limits(
                state_store,
                BLOCKS_PER_BATCH,
                max_apply_batch_blocks,
            ),
        }
    }
}

fn benchmark_mpt_apply_batch_blocks() -> usize {
    std::env::var("NEO_BENCH_MPT_APPLY_BATCH_BLOCKS")
        .ok()
        .and_then(|value| value.parse().ok())
        .filter(|value| *value > 0)
        .unwrap_or(DEFAULT_MPT_APPLY_BATCH_BLOCKS)
}

fn benchmark_prefill_blocks() -> usize {
    std::env::var("NEO_BENCH_PREFILL_BLOCKS")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(0)
}

fn benchmark_stage_metrics_enabled() -> bool {
    std::env::var("NEO_BENCH_STAGE_METRICS")
        .is_ok_and(|value| matches!(value.as_str(), "1" | "true" | "TRUE" | "yes" | "YES"))
}

impl<S: Store> fmt::Debug for StateServiceHooks<S> {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("StateServiceHooks")
            .finish_non_exhaustive()
    }
}

impl<B, S> BlockCommitHooks<B> for StateServiceHooks<S>
where
    B: CacheRead,
    S: Store,
{
    fn block_committing(
        &self,
        block: &Block,
        snapshot: &DataCache<B>,
        _application_executed: &[neo_payloads::ApplicationExecuted],
        _live_tip: u64,
        context: BlockPersistContext,
    ) -> bool {
        if context.skips_live_observers() {
            self.handlers
                .on_committing_deferred(block.index(), snapshot)
        } else {
            self.handlers.on_committing(block.index(), snapshot)
        }
    }

    fn sync_batch_commit_policy(
        &self,
        _start_height: u32,
        _end_height: u32,
        _live_tip: u64,
    ) -> SyncBatchCommitPolicy {
        SyncBatchCommitPolicy::DeferredLive
    }

    fn flush_deferred(&self) -> Result<(), String> {
        self.handlers.flush_result().map_err(str::to_string)
    }

    fn fence_precommit_durability(&self) -> Result<(), String> {
        self.handlers.flush_durable_result()
    }
}

struct ChainCursor {
    next_index: u32,
    next_nonce: u32,
    previous_hash: UInt256,
    timestamp: u64,
    block_interval_ms: u64,
    next_consensus: UInt160,
}

impl ChainCursor {
    fn after_genesis(settings: &ProtocolSettings) -> Self {
        let genesis = neo_blockchain::genesis_block(settings).expect("build benchmark genesis");
        Self {
            next_index: 1,
            next_nonce: 1,
            previous_hash: genesis.hash(),
            timestamp: genesis.timestamp(),
            block_interval_ms: u64::from(settings.milliseconds_per_block),
            next_consensus: *genesis.header.next_consensus(),
        }
    }

    fn gas_transfer_blocks(&mut self, count: usize, sender: &UInt160, script: &[u8]) -> Vec<Block> {
        let mut blocks = Vec::with_capacity(count);
        for _ in 0..count {
            let index = self.next_index;
            let transaction =
                gas_transfer_transaction(self.next_nonce, index, *sender, script.to_vec());
            let mut header = Header::new();
            self.timestamp = self.timestamp.saturating_add(self.block_interval_ms);
            header.set_index(index);
            header.set_prev_hash(self.previous_hash);
            header.set_timestamp(self.timestamp);
            header.set_next_consensus(self.next_consensus);

            let mut block = Block::from_parts(header, vec![transaction]);
            block
                .try_rebuild_merkle_root()
                .expect("rebuild benchmark block Merkle root");
            self.previous_hash = block.hash();
            self.next_index = self.next_index.saturating_add(1);
            self.next_nonce = self.next_nonce.saturating_add(1);
            blocks.push(block);
        }
        blocks
    }
}

fn gas_transfer_script(sender: &UInt160, recipient: &UInt160) -> Vec<u8> {
    let mut builder = ScriptBuilder::new();
    builder.emit_opcode(neo_vm_rs::OpCode::PUSHNULL);
    builder.emit_push_int(TRANSFER_AMOUNT);
    builder.emit_push(&recipient.to_array());
    builder.emit_push(&sender.to_array());
    builder.emit_push_int(4);
    builder.emit_pack();
    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push_string("transfer");
    builder.emit_push(&GasToken::script_hash().to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("emit GAS transfer syscall");
    builder.to_array()
}

fn gas_transfer_transaction(
    nonce: u32,
    block_index: u32,
    sender: UInt160,
    script: Vec<u8>,
) -> Transaction {
    let mut transaction = Transaction::new();
    transaction.set_nonce(nonce);
    transaction.set_valid_until_block(block_index.saturating_add(5_760));
    transaction.set_system_fee(SYSTEM_FEE);
    transaction.set_script(script);
    transaction.set_signers(vec![Signer::new(sender, WitnessScope::GLOBAL)]);
    transaction.set_witnesses(vec![Witness::empty()]);
    transaction
}

#[derive(Default)]
struct ImportTimingTotals {
    blocks: u64,
    transaction: Duration,
    block_clone: Duration,
    ledger_insert: Duration,
    finalized_delivery: Duration,
    finalization: Duration,
    commit_handlers: Duration,
    store_commit: Duration,
}

impl ImportTimingTotals {
    fn record(&mut self, stats: ImportBlocksStats) {
        self.blocks = self.blocks.saturating_add(stats.transaction_blocks as u64);
        self.transaction += stats.transaction_elapsed;
        self.block_clone += stats.transaction_block_clone_elapsed;
        self.ledger_insert += stats.transaction_ledger_insert_elapsed;
        self.finalized_delivery += stats.transaction_finalized_delivery_elapsed;
        self.finalization += stats.finalization_elapsed;
        self.commit_handlers += stats.finalization_commit_handlers_elapsed;
        self.store_commit += stats.finalization_store_commit_elapsed;
    }

    fn print(&self, backend: &str) {
        eprintln!("canonical import stage averages for {backend}:");
        for (stage, elapsed) in [
            ("transaction_persist", self.transaction),
            ("block_clone", self.block_clone),
            ("ledger_insert", self.ledger_insert),
            ("finalized_delivery", self.finalized_delivery),
            ("batch_finalization", self.finalization),
            ("state_service_fence", self.commit_handlers),
            ("ledger_store_commit", self.store_commit),
        ] {
            eprintln!(
                "  {stage:>22}: {:>8} us/block",
                average_us_per_block(elapsed, self.blocks)
            );
        }
    }
}

fn average_us_per_block(elapsed: Duration, blocks: u64) -> u128 {
    if blocks == 0 {
        return 0;
    }
    elapsed.as_micros() / u128::from(blocks)
}

fn print_native_persist_metrics(backend: &str) {
    use neo_runtime::sync_metrics as metrics;

    eprintln!(
        "native persist EWMA for {backend}: total={} us, onpersist={} us, tx={} us, postpersist={} us, cache_commit={} us",
        metrics::native_persist_avg_total_us(),
        metrics::native_persist_avg_onpersist_us(),
        metrics::native_persist_avg_tx_us(),
        metrics::native_persist_avg_postpersist_us(),
        metrics::native_persist_avg_cache_commit_us(),
    );
    for stage in metrics::native_persist_tx_stage_stats() {
        eprintln!(
            "  tx/{:>20}: {:>8} us over {} observations",
            stage.stage, stage.avg_us, stage.calls
        );
    }
    let mut hooks = metrics::native_contract_hook_stats();
    hooks.retain(|hook| hook.calls > 0);
    hooks.sort_unstable_by_key(|hook| std::cmp::Reverse(hook.avg_us));
    for hook in hooks.into_iter().take(6) {
        eprintln!(
            "  {}/{:>20}: {:>8} us over {} observations",
            hook.trigger, hook.contract, hook.avg_us, hook.calls
        );
    }
}

fn print_state_service_metrics(backend: &str) {
    eprintln!("StateService MPT EWMA for {backend}:");
    for stage in StateRootApplyMetrics::state_root_apply_stage_stats() {
        eprintln!(
            "  {:>22}: {:>8} us over {} observations",
            stage.stage, stage.avg_us, stage.calls
        );
    }
    for count in StateRootApplyMetrics::state_root_apply_count_stats() {
        eprintln!(
            "  {:>22}: avg {:>8}, total {} over {} samples",
            count.kind, count.avg, count.total, count.samples
        );
    }
}

struct CanonicalImportFixture<G> {
    runtime: tokio::runtime::Runtime,
    blockchain: BlockchainHandle,
    service_task: Option<tokio::task::JoinHandle<()>>,
    blocks: Vec<Block>,
    _storage_guard: G,
}

impl<G> Drop for CanonicalImportFixture<G> {
    fn drop(&mut self) {
        let _ = self.runtime.block_on(self.blockchain.shutdown());
        if let Some(service_task) = self.service_task.take() {
            let _ = self.runtime.block_on(service_task);
        }
    }
}

fn prepare_canonical_import_fixture<S, T, G>(
    canonical_store: Arc<S>,
    state_store: Arc<StateStore<T>>,
    storage_guard: G,
    mpt_apply_batch_blocks: usize,
    prefill_blocks: usize,
) -> CanonicalImportFixture<G>
where
    S: TransactionalStore,
    T: Store,
{
    let settings = Arc::new(ProtocolSettings::default());
    let native_provider = Arc::new(StandardNativeProvider::new());
    let hooks = Arc::new(StateServiceHooks::new(state_store, mpt_apply_batch_blocks));
    let launch = NodeCoreBuilder::new(
        Arc::clone(&settings),
        canonical_store,
        native_provider,
        hooks,
        0,
    )
    .build();
    let (core, blockchain_task) = launch.into_parts();
    let blockchain = core.blockchain();
    let snapshot = core.snapshot();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .expect("build benchmark Tokio runtime");
    let service_task = runtime.spawn(blockchain_task.run());
    runtime
        .block_on(blockchain.initialize())
        .expect("initialize benchmark blockchain");

    let sender = UInt160::from([0x33; 20]);
    let recipient = UInt160::from([0x44; 20]);
    GasToken::new()
        .fast_forward_mint_state(
            snapshot.as_ref(),
            &sender,
            &BigInt::from(INITIAL_GAS_BALANCE),
        )
        .expect("fund benchmark sender");
    let transfer_script = gas_transfer_script(&sender, &recipient);
    let mut cursor = ChainCursor::after_genesis(settings.as_ref());

    let warmup = cursor.gas_transfer_blocks(1, &sender, &transfer_script);
    let warmup_reply = runtime
        .block_on(blockchain.import_blocks_bulk_detailed(warmup, false))
        .expect("import benchmark warmup block");
    assert_eq!(warmup_reply.imported, 1);
    assert!(warmup_reply.error.is_none(), "{:?}", warmup_reply.error);
    assert_eq!(
        GasToken::balance_of(snapshot.as_ref(), &recipient).expect("read recipient GAS balance"),
        BigInt::from(TRANSFER_AMOUNT),
        "benchmark GAS transfer must execute successfully before measurement"
    );

    let mut remaining_prefill = prefill_blocks;
    while remaining_prefill > 0 {
        let count = remaining_prefill.min(BLOCKS_PER_BATCH);
        let blocks = cursor.gas_transfer_blocks(count, &sender, &transfer_script);
        let reply = runtime
            .block_on(blockchain.import_blocks_bulk_detailed(blocks, false))
            .expect("import benchmark prefill batch");
        assert_eq!(reply.imported, count);
        assert!(reply.error.is_none(), "{:?}", reply.error);
        remaining_prefill -= count;
    }

    CanonicalImportFixture {
        runtime,
        blockchain,
        service_task: Some(service_task),
        blocks: cursor.gas_transfer_blocks(BLOCKS_PER_BATCH, &sender, &transfer_script),
        _storage_guard: storage_guard,
    }
}

fn benchmark_canonical_import<S, T, G, F>(
    bencher: &mut Bencher<'_>,
    backend: &str,
    mpt_apply_batch_blocks: usize,
    prefill_blocks: usize,
    mut storage_factory: F,
) where
    S: TransactionalStore,
    T: Store,
    F: FnMut() -> (Arc<S>, Arc<StateStore<T>>, G),
{
    let mut timings = ImportTimingTotals::default();
    bencher.iter_batched(
        || {
            let (canonical_store, state_store, storage_guard) = storage_factory();
            prepare_canonical_import_fixture(
                canonical_store,
                state_store,
                storage_guard,
                mpt_apply_batch_blocks,
                prefill_blocks,
            )
        },
        |mut fixture| {
            let blocks = std::mem::take(&mut fixture.blocks);
            let reply = fixture
                .runtime
                .block_on(
                    fixture
                        .blockchain
                        .import_blocks_bulk_detailed(blocks, false),
                )
                .expect("import transaction-bearing benchmark batch");
            assert_eq!(reply.imported, BLOCKS_PER_BATCH);
            assert!(reply.error.is_none(), "{:?}", reply.error);
            assert_eq!(reply.stats.transaction_blocks, BLOCKS_PER_BATCH);
            timings.record(reply.stats);
            fixture
        },
        BatchSize::PerIteration,
    );
    if benchmark_stage_metrics_enabled() {
        timings.print(backend);
        print_native_persist_metrics(backend);
        print_state_service_metrics(backend);
    }
}

fn bench_canonical_transaction_import(c: &mut Criterion) {
    let mut group = c.benchmark_group("block_import/canonical_gas_transfer_batches");
    group.throughput(criterion::Throughput::Elements(BLOCKS_PER_BATCH as u64));
    let mpt_apply_batch_blocks = benchmark_mpt_apply_batch_blocks();
    let prefill_blocks = benchmark_prefill_blocks();

    group.bench_with_input(
        BenchmarkId::new(
            format!("memory_blocks_mpt_{mpt_apply_batch_blocks}_prefill_{prefill_blocks}"),
            BLOCKS_PER_BATCH,
        ),
        &(),
        |bencher, ()| {
            benchmark_canonical_import(
                bencher,
                "memory",
                mpt_apply_batch_blocks,
                prefill_blocks,
                || {
                    (
                        Arc::new(MemoryStore::new()),
                        Arc::new(StateStore::with_mpt(false)),
                        (),
                    )
                },
            );
        },
    );

    group.bench_with_input(
        BenchmarkId::new(
            format!("mdbx_blocks_mpt_{mpt_apply_batch_blocks}_prefill_{prefill_blocks}"),
            BLOCKS_PER_BATCH,
        ),
        &(),
        |bencher, ()| {
            benchmark_canonical_import(
                bencher,
                "mdbx",
                mpt_apply_batch_blocks,
                prefill_blocks,
                || {
                    let (canonical_store, canonical_tempdir) =
                        make_mdbx_store("neo-canonical-import-mdbx-bench");
                    let (state_backing, state_tempdir) =
                        make_mdbx_store("neo-canonical-state-service-mdbx-bench");
                    let state_store = Arc::new(
                        StateStore::<MdbxStore>::with_mpt_store(false, state_backing)
                            .expect("create MDBX-backed benchmark StateService"),
                    );
                    (
                        canonical_store,
                        state_store,
                        (canonical_tempdir, state_tempdir),
                    )
                },
            );
        },
    );

    group.finish();
}

criterion_group!(benches, bench_canonical_transaction_import);
criterion_main!(benches);
