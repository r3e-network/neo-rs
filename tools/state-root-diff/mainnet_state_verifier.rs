//! Resumable, persistent, two-upstream MainNet block and state-root verifier.

use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose, Engine as _};
use clap::{Parser, ValueEnum};
use neo_io::{MemoryReader, Serializable};
use neo_primitives::{BlockLike, UInt256};
use neo_protocol::payloads::Block;
use neo_protocol::ProtocolSettings;
use neo_system::ExecutionMode;
use neo_validator::executor::{BlockExecutor, NeoExecutor};
use neo_validator::upstream::{validate_protocol_identity, ExecutionReceipt};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessExt, System, SystemExt};

#[derive(Debug, Clone, Copy, ValueEnum)]
enum ExecutionModeArg {
    Canonical,
    Auto,
    Verify,
}

impl From<ExecutionModeArg> for ExecutionMode {
    fn from(value: ExecutionModeArg) -> Self {
        match value {
            ExecutionModeArg::Canonical => Self::Canonical,
            ExecutionModeArg::Auto => Self::Auto,
            ExecutionModeArg::Verify => Self::Verify,
        }
    }
}

#[derive(Debug, Parser)]
#[command(about = "Verify every MainNet block against two independent RPC upstreams")]
struct Args {
    /// Read an uncompressed official chain.N.acc file and replay it without RPC upstreams.
    /// Offline mode proves local execution only; it does not claim cross-implementation parity.
    #[arg(long)]
    offline_package: Option<PathBuf>,
    /// Explicitly reconstruct a short missing audit tail from the package and persisted roots.
    #[arg(long)]
    repair_offline_audit: bool,
    /// Stop cleanly after this many seconds at an offline batch boundary.
    #[arg(long)]
    offline_run_seconds: Option<u64>,
    #[arg(long, default_value = "http://seed1.neo.org:10332")]
    primary: String,
    #[arg(long, default_value = "http://seed2.neo.org:10332")]
    secondary: String,
    #[arg(long, default_value = "reports/mainnet/full-validation/store")]
    data_path: PathBuf,
    #[arg(long, default_value = "reports/mainnet/full-validation/verified.jsonl")]
    audit_path: PathBuf,
    /// Inclusive target height. Defaults to the common upstream tip at startup.
    #[arg(long)]
    end: Option<u32>,
    #[arg(long, default_value_t = 32)]
    batch_size: u32,
    #[arg(long, value_enum, default_value_t = ExecutionModeArg::Canonical)]
    execution_mode: ExecutionModeArg,
    /// Also compare OnPersist/PostPersist/transaction receipts from both upstreams with neo-rs.
    #[arg(long)]
    verify_receipts: bool,
    /// Disable bounded next-batch prefetch; intended for serial A/B profiling.
    #[arg(long)]
    no_prefetch: bool,
    /// Measure internal persistence stages without changing execution semantics.
    #[arg(long)]
    profile_execution: bool,
    /// Keep WAL enabled, sync once per batch, then flush that batch's audit records.
    /// Interruptions between batch boundaries are rejected on restart.
    #[arg(long)]
    batch_durable: bool,
    /// Disable the flat write-back cache for consecutive empty offline blocks.
    /// Intended for correctness differential tests and serial A/B profiling.
    #[arg(long)]
    disable_empty_block_batch: bool,
    /// Bypass the MPT compact-node cache for a cold, offline A/B run.
    #[arg(long)]
    disable_mpt_node_cache: bool,
    /// Reset the MPT cache to this nonzero capacity before replay (cold A/B only).
    #[arg(long)]
    mpt_node_cache_capacity: Option<std::num::NonZeroUsize>,
    /// Allow an empty audit file to start at an already verified store checkpoint.
    /// The checkpoint block and state root are still checked against both upstreams.
    #[arg(long)]
    new_audit_at_checkpoint: bool,
    #[arg(long, default_value_t = 45)]
    timeout_seconds: u64,
    #[arg(long, default_value_t = 256 * 1024 * 1024)]
    max_response_bytes: usize,
}

#[derive(Clone)]
struct RpcEndpoint {
    name: &'static str,
    url: String,
    client: reqwest::Client,
    next_id: Arc<AtomicU64>,
    max_response_bytes: usize,
}

type ExpectedRequests = HashMap<u64, (String, &'static str)>;

impl RpcEndpoint {
    fn new(
        name: &'static str,
        url: String,
        timeout: Duration,
        max_response_bytes: usize,
    ) -> Result<Self> {
        let client = reqwest::Client::builder()
            .timeout(timeout)
            .build()
            .context("failed to construct RPC client")?;
        Ok(Self {
            name,
            url,
            client,
            next_id: Arc::new(AtomicU64::new(1)),
            max_response_bytes,
        })
    }

    async fn call_one(&self, method: &'static str, params: Value) -> Result<Value> {
        let mut values = self
            .call_batch(vec![("one".to_owned(), method, params)])
            .await?;
        Ok(values
            .remove("one")
            .expect("call_batch preserves every requested key"))
    }

    async fn call_batch(
        &self,
        calls: Vec<(String, &'static str, Value)>,
    ) -> Result<HashMap<String, Value>> {
        let mut last_error = None;
        for attempt in 0..4 {
            match self.call_batch_once(&calls).await {
                Ok(values) => return Ok(values),
                Err(error) => {
                    last_error = Some(error);
                    if attempt < 3 {
                        tokio::time::sleep(Duration::from_millis(250 * (attempt + 1))).await;
                    }
                }
            }
        }
        Err(last_error.expect("at least one RPC attempt was made"))
            .with_context(|| format!("{} RPC batch failed after four attempts", self.name))
    }

    async fn call_batch_once(
        &self,
        calls: &[(String, &'static str, Value)],
    ) -> Result<HashMap<String, Value>> {
        let mut expected = HashMap::with_capacity(calls.len());
        let mut request = Vec::with_capacity(calls.len());
        for (key, method, params) in calls {
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            expected.insert(id, (key.clone(), *method));
            request.push(json!({"jsonrpc":"2.0", "id":id, "method":method, "params":params}));
        }
        let response = self
            .client
            .post(&self.url)
            .json(&request)
            .send()
            .await
            .with_context(|| format!("{} HTTP request failed", self.name))?;
        let status = response.status();
        if response
            .content_length()
            .is_some_and(|length| length > self.max_response_bytes as u64)
        {
            bail!("{} response exceeds configured byte limit", self.name);
        }
        let bytes = response
            .bytes()
            .await
            .with_context(|| format!("{} response body read failed", self.name))?;
        if bytes.len() > self.max_response_bytes {
            bail!("{} response exceeds configured byte limit", self.name);
        }
        if !status.is_success() {
            bail!("{} returned HTTP {status}", self.name);
        }
        let payload: Value = serde_json::from_slice(&bytes)
            .with_context(|| format!("{} returned invalid JSON", self.name))?;
        parse_batch_response(payload, &expected)
            .with_context(|| format!("{} returned an invalid RPC batch", self.name))
    }

    async fn block_count(&self) -> Result<u32> {
        strict_u32(
            &self.call_one("getblockcount", json!([])).await?,
            "block count",
        )
    }

    async fn evidence(
        &self,
        start: u32,
        end: u32,
        verify_receipts: bool,
    ) -> Result<Vec<UpstreamEvidence>> {
        let mut calls = Vec::with_capacity(((end - start + 1) * 2) as usize);
        for height in start..=end {
            calls.push((
                format!("block:{height}"),
                "getblock",
                json!([height, false]),
            ));
            calls.push((format!("root:{height}"), "getstateroot", json!([height])));
        }
        let mut values = self.call_batch(calls).await?;
        let mut evidence = (start..=end)
            .map(|height| {
                let encoded = values
                    .remove(&format!("block:{height}"))
                    .context("block response missing")?;
                let encoded = encoded.as_str().context("getblock result is not Base64")?;
                let raw_block = general_purpose::STANDARD
                    .decode(encoded)
                    .context("getblock result is not canonical Base64")?;
                let root = values
                    .remove(&format!("root:{height}"))
                    .context("state-root response missing")?;
                let root_height = strict_u32(
                    root.get("index").context("state-root index missing")?,
                    "state-root index",
                )?;
                if root_height != height {
                    bail!("state-root index {root_height} does not match requested {height}");
                }
                let state_root =
                    parse_hash(root.get("roothash").and_then(Value::as_str), "state root")?;
                Ok(UpstreamEvidence {
                    height,
                    raw_block,
                    state_root,
                    executions: None,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        if verify_receipts {
            let decoded = evidence
                .iter()
                .map(|item| {
                    let block = decode_block(&item.raw_block, item.height)?;
                    let block_hash = block.hash();
                    let transaction_hashes = block
                        .transactions
                        .iter()
                        .map(|transaction| transaction.hash())
                        .collect::<Vec<_>>();
                    Ok((block_hash, transaction_hashes))
                })
                .collect::<Result<Vec<_>>>()?;
            let mut calls = Vec::new();
            for (item, (block_hash, transaction_hashes)) in evidence.iter().zip(&decoded) {
                calls.push((
                    format!("log:{}", item.height),
                    "getapplicationlog" as &'static str,
                    json!([block_hash.to_string()]),
                ));
                for (index, transaction_hash) in transaction_hashes.iter().enumerate() {
                    calls.push((
                        format!("tx:{}:{index}", item.height),
                        "getapplicationlog" as &'static str,
                        json!([transaction_hash.to_string()]),
                    ));
                }
            }
            let mut logs = self.call_batch(calls).await?;
            for (item, (block_hash, transaction_hashes)) in evidence.iter_mut().zip(decoded) {
                let log = logs
                    .remove(&format!("log:{}", item.height))
                    .context("block application-log response missing")?;
                let system = parse_application_log(&log, block_hash)?;
                if system.len() != 2
                    || system[0].trigger != "OnPersist"
                    || system[1].trigger != "PostPersist"
                {
                    bail!(
                        "block {} system log must contain exactly OnPersist and PostPersist",
                        item.height
                    );
                }
                let mut executions = Vec::with_capacity(system.len() + transaction_hashes.len());
                executions.push(system[0].clone());
                for (index, transaction_hash) in transaction_hashes.iter().enumerate() {
                    let log = logs
                        .remove(&format!("tx:{}:{index}", item.height))
                        .context("transaction application-log response missing")?;
                    let transaction = parse_transaction_log(&log, *transaction_hash)?;
                    if transaction.len() != 1 || transaction[0].trigger != "Application" {
                        bail!(
                            "transaction {} in block {} must have exactly one Application receipt",
                            transaction_hash,
                            item.height
                        );
                    }
                    executions.extend(transaction);
                }
                executions.push(system[1].clone());
                item.executions = Some(executions);
            }
        }
        Ok(evidence)
    }
}

#[derive(Debug)]
struct UpstreamEvidence {
    height: u32,
    raw_block: Vec<u8>,
    state_root: UInt256,
    executions: Option<Vec<ExecutionReceipt>>,
}

#[derive(Debug, Deserialize, Serialize)]
struct AuditRecord {
    height: u32,
    block_hash: String,
    state_root: String,
    #[serde(default)]
    transaction_count: usize,
    #[serde(default)]
    receipts_verified: bool,
    #[serde(default)]
    receipt_count: usize,
    /// Length-prefix offset of this block in an official .acc file.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offline_block_offset: Option<u64>,
    /// First byte after this block; used for exact, fail-closed offline resume.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    offline_next_offset: Option<u64>,
}

#[derive(Debug, Default, Serialize)]
struct ReplayProfile {
    upstream_seconds: f64,
    upstream_wait_seconds: f64,
    decode_compare_seconds: f64,
    local_execution_seconds: f64,
    receipt_compare_seconds: f64,
    durable_flush_seconds: f64,
    audit_seconds: f64,
}

impl ReplayProfile {
    fn measured_seconds(&self) -> f64 {
        self.upstream_seconds
            + self.decode_compare_seconds
            + self.local_execution_seconds
            + self.receipt_compare_seconds
            + self.durable_flush_seconds
            + self.audit_seconds
    }
}

#[derive(Debug, Serialize)]
struct ExecutionStageProfile {
    enabled: bool,
    blocks: u64,
    transaction_executions: u64,
    snapshot_open_seconds: f64,
    transaction_hash_seconds: f64,
    on_persist_seconds: f64,
    transaction_loop_seconds: f64,
    transaction_vm_seconds: f64,
    post_persist_seconds: f64,
    participant_precommit_seconds: f64,
    state_change_extraction_seconds: f64,
    mpt_seconds: f64,
    mpt_update_seconds: f64,
    mpt_commit_seconds: f64,
    mpt_root_hash_seconds: f64,
    mpt_hash_seconds: f64,
    mpt_serialization_seconds: f64,
    mpt_hash256_seconds: f64,
    mpt_node_cache_seconds: f64,
    mpt_node_read_seconds: f64,
    mpt_node_deserialize_seconds: f64,
    mpt_node_encode_seconds: f64,
    mpt_node_write_seconds: f64,
    store_commit_seconds: f64,
    store_writer_snapshot_open_seconds: f64,
    store_change_application_seconds: f64,
    store_write_batch_build_seconds: f64,
    store_database_write_seconds: f64,
    store_cache_invalidation_seconds: f64,
    runtime_update_seconds: f64,
    post_commit_seconds: f64,
    /// Aggregate stages restricted to zero-transaction blocks.
    empty_blocks: EmptyBlockStageProfile,
    /// Nested host wall-clock timings, keyed by Neo's four-byte syscall hash.
    syscalls: Vec<SyscallStageProfile>,
    /// Inclusive dynamic contract calls; nested calls overlap their callers.
    contract_calls: Vec<ContractCallStageProfile>,
    opcodes: Vec<OpcodeStageProfile>,
    external_instructions: u64,
}

#[derive(Debug, Serialize)]
struct EmptyBlockStageProfile {
    blocks: u64,
    snapshot_open_seconds: f64,
    on_persist_seconds: f64,
    transaction_loop_seconds: f64,
    post_persist_seconds: f64,
    participant_precommit_seconds: f64,
    state_change_extraction_seconds: f64,
    mpt_seconds: f64,
    mpt_update_seconds: f64,
    mpt_commit_seconds: f64,
    mpt_hash_seconds: f64,
    mpt_serialization_seconds: f64,
    mpt_hash256_seconds: f64,
    mpt_node_cache_seconds: f64,
    mpt_node_read_seconds: f64,
    mpt_node_deserialize_seconds: f64,
    mpt_node_encode_seconds: f64,
    mpt_node_write_seconds: f64,
    store_commit_seconds: f64,
    store_database_write_seconds: f64,
    runtime_update_seconds: f64,
    post_commit_seconds: f64,
}

#[derive(Debug, Serialize)]
struct OpcodeStageProfile {
    opcode: String,
    count: u64,
}

#[derive(Debug, Serialize)]
struct SyscallStageProfile {
    hash: String,
    calls: u64,
    elapsed_seconds: f64,
}

#[derive(Debug, Serialize)]
struct ContractCallStageProfile {
    contract_hash: String,
    method: String,
    argument_count: usize,
    calls: u64,
    elapsed_seconds: f64,
}

impl From<neo_system::ExecutionProfileSnapshot> for ExecutionStageProfile {
    fn from(profile: neo_system::ExecutionProfileSnapshot) -> Self {
        Self {
            enabled: profile.enabled,
            blocks: profile.blocks,
            transaction_executions: profile.transaction_executions,
            snapshot_open_seconds: profile.snapshot_open.as_secs_f64(),
            transaction_hash_seconds: profile.transaction_hash.as_secs_f64(),
            on_persist_seconds: profile.on_persist.as_secs_f64(),
            transaction_loop_seconds: profile.transaction_loop.as_secs_f64(),
            transaction_vm_seconds: profile.transaction_vm.as_secs_f64(),
            post_persist_seconds: profile.post_persist.as_secs_f64(),
            participant_precommit_seconds: profile.participant_precommit.as_secs_f64(),
            state_change_extraction_seconds: profile.state_change_extraction.as_secs_f64(),
            mpt_seconds: profile.mpt.as_secs_f64(),
            mpt_update_seconds: profile.mpt_update.as_secs_f64(),
            mpt_commit_seconds: profile.mpt_commit.as_secs_f64(),
            mpt_root_hash_seconds: profile.mpt_root_hash.as_secs_f64(),
            mpt_hash_seconds: profile.mpt_hash.as_secs_f64(),
            mpt_serialization_seconds: profile.mpt_serialization.as_secs_f64(),
            mpt_hash256_seconds: profile.mpt_hash256.as_secs_f64(),
            mpt_node_cache_seconds: profile.mpt_node_cache.as_secs_f64(),
            mpt_node_read_seconds: profile.mpt_node_read.as_secs_f64(),
            mpt_node_deserialize_seconds: profile.mpt_node_deserialize.as_secs_f64(),
            mpt_node_encode_seconds: profile.mpt_node_encode.as_secs_f64(),
            mpt_node_write_seconds: profile.mpt_node_write.as_secs_f64(),
            store_commit_seconds: profile.store_commit.as_secs_f64(),
            store_writer_snapshot_open_seconds: profile.store_writer_snapshot_open.as_secs_f64(),
            store_change_application_seconds: profile.store_change_application.as_secs_f64(),
            store_write_batch_build_seconds: profile.store_write_batch_build.as_secs_f64(),
            store_database_write_seconds: profile.store_database_write.as_secs_f64(),
            store_cache_invalidation_seconds: profile.store_cache_invalidation.as_secs_f64(),
            runtime_update_seconds: profile.runtime_update.as_secs_f64(),
            post_commit_seconds: profile.post_commit.as_secs_f64(),
            empty_blocks: EmptyBlockStageProfile {
                blocks: profile.empty_blocks.blocks,
                snapshot_open_seconds: profile.empty_blocks.snapshot_open.as_secs_f64(),
                on_persist_seconds: profile.empty_blocks.on_persist.as_secs_f64(),
                transaction_loop_seconds: profile.empty_blocks.transaction_loop.as_secs_f64(),
                post_persist_seconds: profile.empty_blocks.post_persist.as_secs_f64(),
                participant_precommit_seconds: profile
                    .empty_blocks
                    .participant_precommit
                    .as_secs_f64(),
                state_change_extraction_seconds: profile
                    .empty_blocks
                    .state_change_extraction
                    .as_secs_f64(),
                mpt_seconds: profile.empty_blocks.mpt.as_secs_f64(),
                mpt_update_seconds: profile.empty_blocks.mpt_update.as_secs_f64(),
                mpt_commit_seconds: profile.empty_blocks.mpt_commit.as_secs_f64(),
                mpt_hash_seconds: profile.empty_blocks.mpt_hash.as_secs_f64(),
                mpt_serialization_seconds: profile.empty_blocks.mpt_serialization.as_secs_f64(),
                mpt_hash256_seconds: profile.empty_blocks.mpt_hash256.as_secs_f64(),
                mpt_node_cache_seconds: profile.empty_blocks.mpt_node_cache.as_secs_f64(),
                mpt_node_read_seconds: profile.empty_blocks.mpt_node_read.as_secs_f64(),
                mpt_node_deserialize_seconds: profile
                    .empty_blocks
                    .mpt_node_deserialize
                    .as_secs_f64(),
                mpt_node_encode_seconds: profile.empty_blocks.mpt_node_encode.as_secs_f64(),
                mpt_node_write_seconds: profile.empty_blocks.mpt_node_write.as_secs_f64(),
                store_commit_seconds: profile.empty_blocks.store_commit.as_secs_f64(),
                store_database_write_seconds: profile
                    .empty_blocks
                    .store_database_write
                    .as_secs_f64(),
                runtime_update_seconds: profile.empty_blocks.runtime_update.as_secs_f64(),
                post_commit_seconds: profile.empty_blocks.post_commit.as_secs_f64(),
            },
            syscalls: profile
                .syscalls
                .into_iter()
                .map(|syscall| SyscallStageProfile {
                    hash: format!("0x{:08x}", syscall.hash),
                    calls: syscall.calls,
                    elapsed_seconds: syscall.elapsed.as_secs_f64(),
                })
                .collect(),
            contract_calls: profile
                .contract_calls
                .into_iter()
                .map(|call| ContractCallStageProfile {
                    contract_hash: call.contract_hash,
                    method: call.method,
                    argument_count: call.argument_count,
                    calls: call.calls,
                    elapsed_seconds: call.elapsed.as_secs_f64(),
                })
                .collect(),
            opcodes: profile
                .opcodes
                .into_iter()
                .map(|opcode| OpcodeStageProfile {
                    opcode: format!("0x{:02x}", opcode.opcode),
                    count: opcode.count,
                })
                .collect(),
            external_instructions: profile.external_instructions,
        }
    }
}

#[derive(Debug, Serialize)]
struct MptNodeCacheProfile {
    enabled: bool,
    capacity: usize,
    entries: usize,
    hits: u64,
    misses: u64,
    hit_rate_percent: f32,
    warm_root_nodes: usize,
    warm_root_estimated_bytes: usize,
}

impl From<neo_system::state_service::StateRootNodeCacheStats> for MptNodeCacheProfile {
    fn from(stats: neo_system::state_service::StateRootNodeCacheStats) -> Self {
        Self {
            enabled: stats.enabled,
            capacity: stats.capacity,
            entries: stats.entries,
            hits: stats.hits,
            misses: stats.misses,
            hit_rate_percent: stats.hit_rate_percent,
            warm_root_nodes: stats.warm_root_nodes,
            warm_root_estimated_bytes: stats.warm_root_estimated_bytes,
        }
    }
}

#[derive(Clone, Copy, Debug, Serialize)]
struct ProcessMemoryProfile {
    initial_rss_bytes: u64,
    final_rss_bytes: u64,
    peak_rss_bytes: u64,
    samples: u64,
}

struct ProcessMemoryMonitor {
    system: System,
    pid: Pid,
    initial_rss_bytes: Option<u64>,
    final_rss_bytes: u64,
    peak_rss_bytes: u64,
    samples: u64,
}

impl ProcessMemoryMonitor {
    fn new() -> Result<Self> {
        let pid = sysinfo::get_current_pid()
            .map_err(|error| anyhow::anyhow!("failed to resolve current process id: {error}"))?;
        let mut monitor = Self {
            system: System::new_all(),
            pid,
            initial_rss_bytes: None,
            final_rss_bytes: 0,
            peak_rss_bytes: 0,
            samples: 0,
        };
        monitor.sample()?;
        Ok(monitor)
    }

    fn sample(&mut self) -> Result<u64> {
        self.system.refresh_process(self.pid);
        let rss = self
            .system
            .process(self.pid)
            .context("current process is unavailable for RSS sampling")?
            .memory();
        if rss == 0 {
            bail!("current process returned a zero RSS sample");
        }
        self.initial_rss_bytes.get_or_insert(rss);
        self.final_rss_bytes = rss;
        self.peak_rss_bytes = self.peak_rss_bytes.max(rss);
        self.samples += 1;
        Ok(rss)
    }

    fn profile(&self) -> ProcessMemoryProfile {
        ProcessMemoryProfile {
            initial_rss_bytes: self.initial_rss_bytes.unwrap_or(self.final_rss_bytes),
            final_rss_bytes: self.final_rss_bytes,
            peak_rss_bytes: self.peak_rss_bytes,
            samples: self.samples,
        }
    }
}

#[derive(Debug, Serialize)]
struct RunSummary {
    network: String,
    execution_mode: String,
    receipts_verified: bool,
    prefetch: bool,
    batch_durable: bool,
    new_audit_at_checkpoint: bool,
    mpt_node_cache: MptNodeCacheProfile,
    process_memory: Option<ProcessMemoryProfile>,
    execution_profile: Option<ExecutionStageProfile>,
    initial_height: Option<u32>,
    verified_through: Option<u32>,
    target_height: u32,
    blocks_executed_this_run: u64,
    execution_receipts_this_run: u64,
    direct_transfers_this_run: u64,
    elapsed_seconds: f64,
    replay_profile: ReplayProfile,
    overlapped_seconds: f64,
    unaccounted_seconds: f64,
    complete_for_target: bool,
    primary: String,
    secondary: String,
}

#[tokio::main(flavor = "multi_thread")]
async fn main() -> Result<()> {
    let args = Args::parse();
    if args.batch_size == 0 || args.max_response_bytes == 0 || args.timeout_seconds == 0 {
        bail!("batch size, response limit and timeout must be positive");
    }
    if let Some(package) = args.offline_package.clone() {
        if args.repair_offline_audit {
            return repair_offline_audit(&args, &package);
        }
        return run_offline(args, &package).await;
    }
    if args.repair_offline_audit {
        bail!("--repair-offline-audit requires --offline-package");
    }
    if args.offline_run_seconds.is_some() {
        bail!("--offline-run-seconds requires --offline-package");
    }
    if args.primary == args.secondary {
        bail!("primary and secondary must be different RPC endpoints");
    }
    let primary = RpcEndpoint::new(
        "primary",
        args.primary.clone(),
        Duration::from_secs(args.timeout_seconds),
        args.max_response_bytes,
    )?;
    let secondary = RpcEndpoint::new(
        "secondary",
        args.secondary.clone(),
        Duration::from_secs(args.timeout_seconds),
        args.max_response_bytes,
    )?;
    let settings = ProtocolSettings::mainnet();
    let (primary_identity, secondary_identity, primary_count, secondary_count) = tokio::join!(
        primary.call_one("getversion", json!([])),
        secondary.call_one("getversion", json!([])),
        primary.block_count(),
        secondary.block_count(),
    );
    validate_identity(primary_identity?, &settings)?;
    validate_identity(secondary_identity?, &settings)?;
    let primary_count = primary_count?;
    let secondary_count = secondary_count?;
    if primary_count == 0 || secondary_count == 0 {
        bail!("an upstream returned an empty chain");
    }
    let common_tip = primary_count.min(secondary_count) - 1;
    let target = args.end.unwrap_or(common_tip).min(common_tip);
    let executor = NeoExecutor::open_with_batch_durability(
        settings.clone(),
        &args.data_path,
        args.batch_durable,
    )?;
    executor.set_execution_mode(args.execution_mode.into());
    executor.set_execution_profiling(args.profile_execution);
    executor.set_mpt_node_cache_enabled(!args.disable_mpt_node_cache);
    if let Some(capacity) = args.mpt_node_cache_capacity {
        executor.reset_mpt_node_cache_capacity(capacity);
    }
    let mut process_memory = args
        .profile_execution
        .then(ProcessMemoryMonitor::new)
        .transpose()?;
    let initial_height = executor.height()?;
    if initial_height.is_some_and(|height| height > target) {
        bail!("local height {initial_height:?} exceeds requested target {target}");
    }
    let checkpoint = executor.checkpoint()?;
    if let Some(checkpoint) = checkpoint.as_ref() {
        let (primary, secondary) = tokio::join!(
            primary.evidence(checkpoint.height, checkpoint.height, args.verify_receipts,),
            secondary.evidence(checkpoint.height, checkpoint.height, args.verify_receipts,),
        );
        let mut primary = primary?;
        let mut secondary = secondary?;
        if primary.len() != 1 || secondary.len() != 1 {
            bail!("restart evidence must contain exactly one block per upstream");
        }
        let evidence = compare_evidence(primary.remove(0), secondary.remove(0))?;
        let block = decode_block(&evidence.raw_block, checkpoint.height)?;
        if block.hash() != checkpoint.block_hash {
            bail!("restart block hash differs from both upstreams");
        }
        if evidence.state_root != checkpoint.state_root {
            bail!("restart state root differs from both upstreams");
        }
    }

    if let Some(parent) = args.audit_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut audit = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(&args.audit_path)
        .with_context(|| format!("failed to open {}", args.audit_path.display()))?;
    validate_audit_resume_mode(
        &mut audit,
        checkpoint.as_ref(),
        args.verify_receipts,
        args.new_audit_at_checkpoint,
    )?;
    let mut audit = BufWriter::new(audit);
    let started = Instant::now();
    let mut executed = 0_u64;
    let mut receipts = 0_u64;
    let mut replay_profile = ReplayProfile::default();
    let next = initial_height.map_or(0, |height| height + 1);
    let output_dir = args.audit_path.parent().unwrap_or_else(|| Path::new("."));
    let mismatch_path = output_dir.join("mismatch.json");
    let result = verify_range(
        &executor,
        &primary,
        &secondary,
        next,
        target,
        args.batch_size,
        args.verify_receipts,
        !args.no_prefetch,
        args.batch_durable,
        &mismatch_path,
        &mut audit,
        &mut executed,
        &mut receipts,
        &mut replay_profile,
        &mut process_memory,
    )
    .await;
    audit.flush()?;
    let verified_through = executor.height()?;
    let elapsed_seconds = started.elapsed().as_secs_f64();
    let measured_seconds = replay_profile.measured_seconds();
    let overlapped_seconds = (measured_seconds - elapsed_seconds).max(0.0);
    let unaccounted_seconds = (elapsed_seconds - measured_seconds).max(0.0);
    if let Some(monitor) = process_memory.as_mut() {
        monitor.sample()?;
    }
    let summary = RunSummary {
        network: "mainnet".to_owned(),
        execution_mode: format!("{:?}", args.execution_mode).to_lowercase(),
        receipts_verified: args.verify_receipts,
        prefetch: !args.no_prefetch,
        batch_durable: args.batch_durable,
        new_audit_at_checkpoint: args.new_audit_at_checkpoint,
        mpt_node_cache: executor.mpt_node_cache_stats().into(),
        process_memory: process_memory.as_ref().map(ProcessMemoryMonitor::profile),
        execution_profile: args
            .profile_execution
            .then(|| executor.execution_profile().into()),
        initial_height,
        verified_through,
        target_height: target,
        blocks_executed_this_run: executed,
        execution_receipts_this_run: receipts,
        direct_transfers_this_run: executor.direct_transfer_count(),
        elapsed_seconds,
        replay_profile,
        overlapped_seconds,
        unaccounted_seconds,
        complete_for_target: result.is_ok() && verified_through == Some(target),
        primary: args.primary,
        secondary: args.secondary,
    };
    let summary_path = output_dir.join("last-run.json");
    fs::write(&summary_path, serde_json::to_vec_pretty(&summary)?)?;
    executor.shutdown().await?;
    result?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

#[derive(Debug)]
struct OfflineBlock {
    height: u32,
    raw: Vec<u8>,
    block_offset: u64,
    next_offset: u64,
}

/// Strict reader for the format implemented by Neo.CLI MainService.Block.cs:
/// uint32 start, uint32 count, then count entries of int32 length + raw Block.
struct OfflineBlockSource {
    file: File,
    file_len: u64,
    start: u32,
    end: u32,
    next_height: u32,
}

impl OfflineBlockSource {
    fn open(path: &Path) -> Result<Self> {
        let mut file = File::open(path)
            .with_context(|| format!("failed to open offline package {}", path.display()))?;
        let file_len = file.metadata()?.len();
        if file_len < 8 {
            bail!("offline package is shorter than its eight-byte header");
        }
        let start = read_u32_le(&mut file, "offline package start")?;
        let count = read_u32_le(&mut file, "offline package count")?;
        if start != 0 || count == 0 {
            bail!("full-chain offline replay requires a non-empty package starting at genesis");
        }
        let end = start
            .checked_add(count - 1)
            .context("offline package end height overflow")?;
        Ok(Self {
            file,
            file_len,
            start,
            end,
            next_height: start,
        })
    }

    fn read_one(&mut self) -> Result<OfflineBlock> {
        if self.next_height > self.end {
            bail!("attempted to read beyond the declared offline package count");
        }
        let height = self.next_height;
        let block_offset = self.file.stream_position()?;
        let size = read_u32_le(&mut self.file, "offline block length")? as usize;
        if size == 0 || size > neo_system::network::p2p::PAYLOAD_MAX_SIZE {
            bail!("offline block {height} has invalid length {size}");
        }
        let mut raw = vec![0_u8; size];
        self.file
            .read_exact(&mut raw)
            .with_context(|| format!("offline block {height} is truncated"))?;
        let next_offset = self.file.stream_position()?;
        if height == self.end && next_offset != self.file_len {
            bail!("offline package has trailing bytes or an incorrect declared count");
        }
        self.next_height = self
            .next_height
            .checked_add(1)
            .context("offline block height overflow")?;
        Ok(OfflineBlock {
            height,
            raw,
            block_offset,
            next_offset,
        })
    }

    fn read_batch(&mut self, start: u32, end: u32) -> Result<Vec<OfflineBlock>> {
        if start > end || start != self.next_height || end > self.end {
            bail!("offline batch is outside the declared sequential block range");
        }
        (start..=end).map(|_| self.read_one()).collect()
    }

    fn resume(
        &mut self,
        checkpoint: &neo_validator::state::VerifiedPoint,
        record: &AuditRecord,
    ) -> Result<()> {
        let block_offset = record
            .offline_block_offset
            .context("offline audit checkpoint has no block offset")?;
        let expected_next = record
            .offline_next_offset
            .context("offline audit checkpoint has no next offset")?;
        self.file.seek(SeekFrom::Start(block_offset))?;
        self.next_height = checkpoint.height;
        let block = self.read_one()?;
        if block.block_offset != block_offset || block.next_offset != expected_next {
            bail!("offline audit offsets do not describe the checkpoint block");
        }
        let decoded = decode_block(&block.raw, checkpoint.height)?;
        if decoded.hash() != checkpoint.block_hash {
            bail!("offline package checkpoint block hash differs from the validator store");
        }
        Ok(())
    }
}

fn read_u32_le(file: &mut File, field: &str) -> Result<u32> {
    let mut bytes = [0_u8; 4];
    file.read_exact(&mut bytes)
        .with_context(|| format!("failed to read {field}"))?;
    Ok(u32::from_le_bytes(bytes))
}

fn read_last_audit_record(file: &mut File) -> Result<AuditRecord> {
    let length = file.metadata()?.len();
    let tail_length = length.min(8192);
    file.seek(SeekFrom::End(-(tail_length as i64)))?;
    let mut tail = String::new();
    file.read_to_string(&mut tail)?;
    let line = tail
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .context("audit file has no final record")?;
    let record = serde_json::from_str(line).context("invalid final audit record")?;
    file.seek(SeekFrom::End(0))?;
    Ok(record)
}

fn read_direct_point(
    cache: &neo_storage::persistence::store_cache::StoreCache,
    height: u32,
) -> Result<neo_validator::state::VerifiedPoint> {
    use neo_native_contracts::ledger_contract::{HashOrIndex, LedgerContract};
    use neo_system::state_service::commit_handlers::read_state_root_record;

    let block = LedgerContract::new()
        .get_block(cache, HashOrIndex::Index(height))?
        .with_context(|| format!("persisted block {height} is missing"))?;
    if block.index() != height {
        bail!("persisted block height differs from its index at {height}");
    }
    let state_root = read_state_root_record(cache, height)
        .map_err(anyhow::Error::msg)?
        .with_context(|| format!("persisted state root {height} is missing"))?;
    Ok(neo_validator::state::VerifiedPoint {
        height,
        block_hash: block.hash(),
        state_root: state_root.root_hash,
    })
}

fn open_direct_checkpoint(
    data_path: &Path,
) -> Result<(
    neo_storage::persistence::store_cache::StoreCache,
    neo_validator::state::VerifiedPoint,
)> {
    use neo_native_contracts::ledger_contract::LedgerContract;
    use neo_storage::persistence::providers::rocksdb::RocksDBStoreProvider;
    use neo_storage::persistence::storage::StorageConfig;
    use neo_storage::persistence::store_cache::StoreCache;
    use neo_storage::persistence::store_provider::StoreProvider;

    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: data_path.to_path_buf(),
        ..StorageConfig::default()
    });
    let store = provider.get_store("")?;
    let cache = StoreCache::new(store);
    let height = LedgerContract::new().current_index(&cache)?;
    let point = read_direct_point(&cache, height)?;
    Ok((cache, point))
}

fn repair_offline_audit(args: &Args, package: &Path) -> Result<()> {
    use std::io::BufRead;

    let (cache, checkpoint) = open_direct_checkpoint(&args.data_path)?;
    let mut file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&args.audit_path)
        .with_context(|| format!("failed to open {} for repair", args.audit_path.display()))?;
    let original_len = file.metadata()?.len();
    let mut reader = std::io::BufReader::new(file.try_clone()?);
    let mut valid_len = 0_u64;
    let mut expected_height = 0_u32;
    let mut last: Option<AuditRecord> = None;
    loop {
        let mut line = Vec::new();
        let read = reader.read_until(b'\n', &mut line)?;
        if read == 0 {
            break;
        }
        if !line.ends_with(b"\n") {
            break;
        }
        let record: AuditRecord = serde_json::from_slice(&line)
            .with_context(|| format!("invalid complete audit record at byte {valid_len}"))?;
        if record.height != expected_height {
            bail!("offline audit is not contiguous at height {expected_height}");
        }
        if record.offline_block_offset.is_none() || record.offline_next_offset.is_none() {
            bail!("non-offline audit record found at height {}", record.height);
        }
        if let Some(previous) = last.as_ref() {
            if record.offline_block_offset != previous.offline_next_offset {
                bail!(
                    "offline audit offsets are not contiguous at {}",
                    record.height
                );
            }
        }
        valid_len = valid_len
            .checked_add(read as u64)
            .context("audit length overflow")?;
        expected_height = expected_height
            .checked_add(1)
            .context("audit height overflow")?;
        last = Some(record);
    }
    drop(reader);
    let last = last.context("offline audit has no complete anchor record")?;
    let anchor = read_direct_point(&cache, last.height)?;
    if anchor.block_hash.to_string() != last.block_hash
        || anchor.state_root.to_string() != last.state_root
    {
        bail!("last complete audit record differs from persisted state");
    }
    if checkpoint.height < last.height {
        bail!("persisted checkpoint is behind the complete audit; refusing to truncate state");
    }
    let missing = checkpoint.height - last.height;
    if missing > 4096 {
        bail!("offline audit gap {missing} exceeds the explicit repair limit");
    }

    let mut repaired = Vec::new();
    if missing > 0 {
        let mut source = OfflineBlockSource::open(package)?;
        source.file.seek(SeekFrom::Start(
            last.offline_next_offset
                .context("offline anchor has no next offset")?,
        ))?;
        source.next_height = last.height + 1;
        for height in (last.height + 1)..=checkpoint.height {
            let item = source.read_one()?;
            let block = decode_block(&item.raw, height)?;
            let point = read_direct_point(&cache, height)?;
            if block.hash() != point.block_hash {
                bail!("package and persisted block hashes differ at {height}");
            }
            let mut encoded = serde_json::to_vec(&AuditRecord {
                height,
                block_hash: point.block_hash.to_string(),
                state_root: point.state_root.to_string(),
                transaction_count: block.transactions.len(),
                receipts_verified: false,
                receipt_count: 0,
                offline_block_offset: Some(item.block_offset),
                offline_next_offset: Some(item.next_offset),
            })?;
            encoded.push(b'\n');
            repaired.push(encoded);
        }
    }
    if valid_len == original_len && repaired.is_empty() {
        println!(
            "{}",
            serde_json::to_string_pretty(&json!({
                "repair": "not_needed",
                "height": checkpoint.height,
            }))?
        );
        return Ok(());
    }

    let file_name = args
        .audit_path
        .file_name()
        .and_then(|name| name.to_str())
        .context("offline audit filename is invalid")?;
    let backup = args
        .audit_path
        .with_file_name(format!("{file_name}.pre-repair-{}", checkpoint.height));
    if backup.exists() {
        bail!("repair backup already exists: {}", backup.display());
    }
    fs::copy(&args.audit_path, &backup)
        .with_context(|| format!("failed to back up audit to {}", backup.display()))?;
    file.set_len(valid_len)?;
    file.seek(SeekFrom::Start(valid_len))?;
    for record in &repaired {
        file.write_all(record)?;
    }
    file.sync_all()?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "repair": "completed",
            "previous_complete_height": last.height,
            "persisted_checkpoint": checkpoint,
            "records_reconstructed": repaired.len(),
            "trailing_bytes_removed": original_len - valid_len,
            "backup": backup,
        }))?
    );
    Ok(())
}

#[derive(Debug, Serialize)]
struct OfflineRunSummary {
    network: String,
    source: String,
    package_start: u32,
    package_end: u32,
    execution_mode: String,
    local_replay_only: bool,
    receipts_verified: bool,
    batch_durable: bool,
    empty_block_write_back: bool,
    mpt_node_cache: MptNodeCacheProfile,
    process_memory: Option<ProcessMemoryProfile>,
    execution_profile: Option<ExecutionStageProfile>,
    initial_height: Option<u32>,
    verified_through: Option<u32>,
    target_height: u32,
    blocks_executed_this_run: u64,
    direct_transfers_this_run: u64,
    elapsed_seconds: f64,
    replay_profile: ReplayProfile,
    complete_for_target: bool,
}

async fn run_offline(args: Args, package: &Path) -> Result<()> {
    if !matches!(args.execution_mode, ExecutionModeArg::Canonical) {
        bail!("offline full-chain replay requires --execution-mode canonical");
    }
    if args.verify_receipts {
        bail!("offline packages do not contain upstream execution receipts");
    }
    if args.new_audit_at_checkpoint {
        bail!("offline replay does not permit skipping audit history at a checkpoint");
    }
    if args.offline_run_seconds == Some(0) {
        bail!("--offline-run-seconds must be positive");
    }
    let mut source = OfflineBlockSource::open(package)?;
    let target = match args.end {
        Some(target) if target > source.end => {
            bail!(
                "requested target {target} exceeds offline package end {}",
                source.end
            )
        }
        Some(target) => target,
        None => source.end,
    };
    let executor = NeoExecutor::open_with_batch_durability_checkpoint_replay(
        ProtocolSettings::mainnet(),
        &args.data_path,
        args.batch_durable,
    )?;
    executor.set_execution_mode(ExecutionMode::Canonical);
    executor.set_execution_profiling(args.profile_execution);
    executor.set_mpt_node_cache_enabled(!args.disable_mpt_node_cache);
    if let Some(capacity) = args.mpt_node_cache_capacity {
        executor.reset_mpt_node_cache_capacity(capacity);
    }
    let mut process_memory = args
        .profile_execution
        .then(ProcessMemoryMonitor::new)
        .transpose()?;
    let initial_height = executor.height()?;
    if initial_height.is_some_and(|height| height > target) {
        bail!("local height {initial_height:?} exceeds requested target {target}");
    }
    let checkpoint = executor.checkpoint()?;
    if initial_height.is_some() != checkpoint.is_some() {
        bail!("validator height and checkpoint availability disagree");
    }
    if let Some(parent) = args.audit_path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut audit = OpenOptions::new()
        .create(true)
        .read(true)
        .append(true)
        .open(&args.audit_path)
        .with_context(|| format!("failed to open {}", args.audit_path.display()))?;
    validate_audit_resume(&mut audit, checkpoint.as_ref(), false)?;
    if let Some(checkpoint) = checkpoint.as_ref() {
        let record = read_last_audit_record(&mut audit)?;
        source.resume(checkpoint, &record)?;
    }
    let mut audit = BufWriter::new(audit);
    let started = Instant::now();
    let mut executed = 0_u64;
    let mut replay_profile = ReplayProfile::default();
    let next = initial_height.map_or(source.start, |height| height + 1);
    let output_dir = args.audit_path.parent().unwrap_or_else(|| Path::new("."));
    let result = replay_offline_range(
        &executor,
        &mut source,
        next,
        target,
        args.batch_size,
        args.batch_durable,
        !args.disable_empty_block_batch,
        args.offline_run_seconds,
        &mut audit,
        &mut executed,
        &mut replay_profile,
        &mut process_memory,
    );
    audit.flush()?;
    if let Err(error) = result {
        // A failed speculative batch poisons the executor. Do not query its
        // in-memory height or emit a success-shaped summary; durable storage
        // remains at the last completed physical batch boundary.
        executor.shutdown().await?;
        return Err(error);
    }
    let verified_through = executor.height()?;
    if let Some(monitor) = process_memory.as_mut() {
        monitor.sample()?;
    }
    let summary = OfflineRunSummary {
        network: "mainnet".to_owned(),
        source: package.display().to_string(),
        package_start: source.start,
        package_end: source.end,
        execution_mode: "canonical".to_owned(),
        local_replay_only: true,
        receipts_verified: false,
        batch_durable: args.batch_durable,
        empty_block_write_back: !args.disable_empty_block_batch,
        mpt_node_cache: executor.mpt_node_cache_stats().into(),
        process_memory: process_memory.as_ref().map(ProcessMemoryMonitor::profile),
        execution_profile: args
            .profile_execution
            .then(|| executor.execution_profile().into()),
        initial_height,
        verified_through,
        target_height: target,
        blocks_executed_this_run: executed,
        direct_transfers_this_run: executor.direct_transfer_count(),
        elapsed_seconds: started.elapsed().as_secs_f64(),
        replay_profile,
        complete_for_target: verified_through == Some(target),
    };
    fs::write(
        output_dir.join("last-run.json"),
        serde_json::to_vec_pretty(&summary)?,
    )?;
    executor.shutdown().await?;
    println!("{}", serde_json::to_string_pretty(&summary)?);
    Ok(())
}

fn replay_offline_range(
    executor: &NeoExecutor,
    source: &mut OfflineBlockSource,
    start: u32,
    target: u32,
    batch_size: u32,
    batch_durable: bool,
    empty_block_write_back: bool,
    run_seconds: Option<u64>,
    audit: &mut impl Write,
    executed: &mut u64,
    profile: &mut ReplayProfile,
    process_memory: &mut Option<ProcessMemoryMonitor>,
) -> Result<()> {
    if start > target {
        return Ok(());
    }
    let started = Instant::now();
    let mut batch_start = start;
    while batch_start <= target {
        let batch_end = batch_start.saturating_add(batch_size - 1).min(target);
        let stage = Instant::now();
        let batch = source.read_batch(batch_start, batch_end)?;
        profile.upstream_seconds += stage.elapsed().as_secs_f64();
        let result = process_offline_batch(
            executor,
            batch,
            !batch_durable,
            empty_block_write_back,
            audit,
            executed,
            profile,
        )
        .and_then(|()| {
            if batch_durable {
                let stage = Instant::now();
                let flushed = executor.flush_durable();
                profile.durable_flush_seconds += stage.elapsed().as_secs_f64();
                flushed?;
            }
            let stage = Instant::now();
            let flushed = audit.flush();
            profile.audit_seconds += stage.elapsed().as_secs_f64();
            flushed.map_err(Into::into)
        });
        result?;
        if let Some(monitor) = process_memory.as_mut() {
            monitor.sample()?;
        }
        println!(
            "offline-verified={batch_end} target={target} batch={batch_start}-{batch_end} executed={} rate={:.2} blocks/s read={:.2}s execute={:.2}s audit={:.2}s",
            *executed,
            *executed as f64 / started.elapsed().as_secs_f64().max(f64::EPSILON),
            profile.upstream_seconds,
            profile.local_execution_seconds,
            profile.audit_seconds,
        );
        batch_start = batch_end.checked_add(1).context("block height overflow")?;
        if run_seconds.is_some_and(|seconds| started.elapsed() >= Duration::from_secs(seconds)) {
            println!("offline-run-time-limit reached at a durable batch boundary");
            break;
        }
    }
    Ok(())
}

fn process_offline_batch(
    executor: &NeoExecutor,
    batch: Vec<OfflineBlock>,
    flush_each_block: bool,
    empty_block_write_back: bool,
    audit: &mut impl Write,
    executed: &mut u64,
    profile: &mut ReplayProfile,
) -> Result<()> {
    let decode_started = Instant::now();
    let mut prepared = Vec::with_capacity(batch.len());
    for item in batch {
        let block = decode_block(&item.raw, item.height)?;
        let block_hash = block.hash();
        prepared.push((item, block, block_hash));
    }
    profile.decode_compare_seconds += decode_started.elapsed().as_secs_f64();

    let mut offset = 0_usize;
    while offset < prepared.len() {
        let is_empty = prepared[offset].1.transactions.is_empty();
        let mut end = offset + 1;
        if empty_block_write_back && is_empty {
            while end < prepared.len()
                && prepared[end].1.transactions.is_empty()
                && end - offset < neo_system::NeoSystem::MAX_EMPTY_BLOCK_WRITE_BACK_BLOCKS
            {
                end += 1;
            }
        }
        let run = &prepared[offset..end];
        let first_height = run[0].0.height;
        let last_height = run[run.len() - 1].0.height;
        let stage = Instant::now();
        let outcomes = if run.len() >= 2 {
            let blocks = run.iter().map(|(_, block, _)| block.clone()).collect();
            executor.execute_empty_blocks(blocks).with_context(|| {
                format!(
                    "neo-rs failed to execute offline empty-block run {first_height}-{last_height}"
                )
            })?
        } else {
            vec![executor.execute(run[0].1.clone()).with_context(|| {
                format!("neo-rs failed to execute offline block {first_height}")
            })?]
        };
        profile.local_execution_seconds += stage.elapsed().as_secs_f64();
        if outcomes.len() != run.len() {
            bail!("offline execution returned the wrong outcome count at {first_height}-{last_height}");
        }

        let stage = Instant::now();
        for ((item, block, block_hash), outcome) in run.iter().zip(outcomes) {
            if outcome.block_hash != *block_hash {
                bail!(
                    "local block hash changed during execution at {}",
                    item.height
                );
            }
            serde_json::to_writer(
                &mut *audit,
                &AuditRecord {
                    height: item.height,
                    block_hash: block_hash.to_string(),
                    state_root: outcome.state_root.to_string(),
                    transaction_count: block.transactions.len(),
                    receipts_verified: false,
                    receipt_count: 0,
                    offline_block_offset: Some(item.block_offset),
                    offline_next_offset: Some(item.next_offset),
                },
            )?;
            audit.write_all(b"\n")?;
            if flush_each_block {
                audit.flush()?;
            }
            *executed += 1;
        }
        profile.audit_seconds += stage.elapsed().as_secs_f64();
        offset = end;
    }
    Ok(())
}

fn validate_identity(identity: Value, settings: &ProtocolSettings) -> Result<()> {
    // Public seed operators may choose a different local block-production interval.
    // Execution-affecting MainNet parameters and hardfork heights must still match.
    let mut expected = settings.clone();
    if let Some(milliseconds) = identity
        .get("protocol")
        .and_then(|protocol| protocol.get("msperblock"))
        .and_then(Value::as_u64)
    {
        expected.milliseconds_per_block = u32::try_from(milliseconds)
            .context("upstream msperblock exceeds the supported range")?;
    }
    validate_protocol_identity(&identity, &expected)
}

struct EvidenceBatch {
    primary: Vec<UpstreamEvidence>,
    secondary: Vec<UpstreamEvidence>,
    fetch_seconds: f64,
}

async fn fetch_batch(
    primary: &RpcEndpoint,
    secondary: &RpcEndpoint,
    start: u32,
    end: u32,
    verify_receipts: bool,
) -> Result<EvidenceBatch> {
    let started = Instant::now();
    let (primary, secondary) = tokio::join!(
        primary.evidence(start, end, verify_receipts),
        secondary.evidence(start, end, verify_receipts),
    );
    let primary = primary?;
    let secondary = secondary?;
    if primary.len() != secondary.len() {
        bail!("upstream evidence batch lengths differ");
    }
    Ok(EvidenceBatch {
        primary,
        secondary,
        fetch_seconds: started.elapsed().as_secs_f64(),
    })
}

fn spawn_fetch(
    primary: &RpcEndpoint,
    secondary: &RpcEndpoint,
    start: u32,
    end: u32,
    verify_receipts: bool,
) -> tokio::task::JoinHandle<Result<EvidenceBatch>> {
    let primary = primary.clone();
    let secondary = secondary.clone();
    tokio::spawn(
        async move { fetch_batch(&primary, &secondary, start, end, verify_receipts).await },
    )
}

async fn verify_range(
    executor: &NeoExecutor,
    primary: &RpcEndpoint,
    secondary: &RpcEndpoint,
    start: u32,
    target: u32,
    batch_size: u32,
    verify_receipts: bool,
    prefetch: bool,
    batch_durable: bool,
    mismatch_path: &Path,
    audit: &mut impl Write,
    executed: &mut u64,
    receipts: &mut u64,
    profile: &mut ReplayProfile,
    process_memory: &mut Option<ProcessMemoryMonitor>,
) -> Result<()> {
    if start > target {
        return Ok(());
    }
    let started = Instant::now();
    let mut batch_start = start;
    let first_end = batch_start.saturating_add(batch_size - 1).min(target);
    let mut pending =
        prefetch.then(|| spawn_fetch(primary, secondary, batch_start, first_end, verify_receipts));
    while batch_start <= target {
        let batch_end = batch_start.saturating_add(batch_size - 1).min(target);
        let stage = Instant::now();
        let batch = match pending.take() {
            Some(task) => task.await.context("upstream fetch task failed")??,
            None => {
                fetch_batch(primary, secondary, batch_start, batch_end, verify_receipts).await?
            }
        };
        profile.upstream_wait_seconds += stage.elapsed().as_secs_f64();
        profile.upstream_seconds += batch.fetch_seconds;
        let mut next_pending = if prefetch && batch_end < target {
            let next_start = batch_end + 1;
            let next_end = next_start.saturating_add(batch_size - 1).min(target);
            Some(spawn_fetch(
                primary,
                secondary,
                next_start,
                next_end,
                verify_receipts,
            ))
        } else {
            None
        };
        let result = process_batch(
            executor,
            batch.primary,
            batch.secondary,
            verify_receipts,
            !batch_durable,
            mismatch_path,
            audit,
            executed,
            receipts,
            profile,
        )
        .and_then(|()| {
            if batch_durable {
                let stage = Instant::now();
                let flushed = executor.flush_durable();
                profile.durable_flush_seconds += stage.elapsed().as_secs_f64();
                flushed?;
            }
            let stage = Instant::now();
            let flushed = audit.flush();
            profile.audit_seconds += stage.elapsed().as_secs_f64();
            flushed.map_err(Into::into)
        });
        if let Err(error) = result {
            if let Some(task) = next_pending.take() {
                task.abort();
                let _ = task.await;
            }
            return Err(error);
        }
        pending = next_pending;
        if let Some(monitor) = process_memory.as_mut() {
            monitor.sample()?;
        }
        let elapsed = started.elapsed().as_secs_f64();
        println!(
            "verified={batch_end} target={target} batch={batch_start}-{batch_end} executed={} receipts={} rate={:.2} blocks/s upstream={:.2}s wait={:.2}s execute={:.2}s audit={:.2}s",
            *executed,
            *receipts,
            *executed as f64 / elapsed.max(f64::EPSILON),
            profile.upstream_seconds,
            profile.upstream_wait_seconds,
            profile.local_execution_seconds,
            profile.audit_seconds,
        );
        batch_start = batch_end.checked_add(1).context("block height overflow")?;
    }
    Ok(())
}

fn process_batch(
    executor: &NeoExecutor,
    primary: Vec<UpstreamEvidence>,
    secondary: Vec<UpstreamEvidence>,
    verify_receipts: bool,
    flush_each_block: bool,
    mismatch_path: &Path,
    audit: &mut impl Write,
    executed: &mut u64,
    receipts: &mut u64,
    profile: &mut ReplayProfile,
) -> Result<()> {
    for (primary, secondary) in primary.into_iter().zip(secondary) {
        let height = primary.height;
        let stage = Instant::now();
        let evidence = compare_evidence(primary, secondary)?;
        let block = decode_block(&evidence.raw_block, height)?;
        let block_hash = block.hash();
        let transaction_count = block.transactions.len();
        profile.decode_compare_seconds += stage.elapsed().as_secs_f64();
        let stage = Instant::now();
        let outcome = executor
            .execute(block)
            .with_context(|| format!("neo-rs failed to execute block {height}"))?;
        profile.local_execution_seconds += stage.elapsed().as_secs_f64();
        if outcome.block_hash != block_hash {
            bail!("local block hash changed during execution at {height}");
        }
        if outcome.state_root != evidence.state_root {
            let incident = json!({
                "height": height,
                "code": "state_root_mismatch",
                "block_hash": block_hash.to_string(),
                "local_state_root": outcome.state_root.to_string(),
                "upstream_state_root": evidence.state_root.to_string(),
            });
            write_mismatch(mismatch_path, audit, &incident)?;
            bail!("state-root mismatch at {height}: {incident}");
        }
        let stage = Instant::now();
        let receipt_count = if verify_receipts {
            match compare_execution_receipts(
                height,
                evidence.executions.as_deref(),
                outcome.executions.as_deref(),
            ) {
                Ok(count) => count,
                Err(error) => {
                    let incident = json!({
                        "height": height,
                        "code": "execution_receipt_mismatch",
                        "block_hash": block_hash.to_string(),
                        "detail": format!("{error:#}"),
                    });
                    write_mismatch(mismatch_path, audit, &incident)?;
                    return Err(error);
                }
            }
        } else {
            0
        };
        profile.receipt_compare_seconds += stage.elapsed().as_secs_f64();
        let stage = Instant::now();
        serde_json::to_writer(
            &mut *audit,
            &AuditRecord {
                height,
                block_hash: block_hash.to_string(),
                state_root: outcome.state_root.to_string(),
                transaction_count,
                receipts_verified: verify_receipts,
                receipt_count,
                offline_block_offset: None,
                offline_next_offset: None,
            },
        )?;
        audit.write_all(b"\n")?;
        // Default mode persists each audit watermark immediately. Batch-durable
        // mode flushes only after the enclosing batch's WAL sync succeeds.
        if flush_each_block {
            audit.flush()?;
        }
        profile.audit_seconds += stage.elapsed().as_secs_f64();
        *executed += 1;
        *receipts = receipts
            .checked_add(u64::try_from(receipt_count)?)
            .context("execution receipt counter overflow")?;
    }
    Ok(())
}

fn compare_evidence(
    primary: UpstreamEvidence,
    secondary: UpstreamEvidence,
) -> Result<UpstreamEvidence> {
    if primary.height != secondary.height {
        bail!("upstream evidence heights differ");
    }
    if primary.raw_block != secondary.raw_block {
        bail!("upstream raw block bytes differ at {}", primary.height);
    }
    if primary.state_root != secondary.state_root {
        bail!("upstream state roots differ at {}", primary.height);
    }
    if primary.executions != secondary.executions {
        bail!("upstream execution receipts differ at {}", primary.height);
    }
    Ok(primary)
}

fn compare_execution_receipts(
    height: u32,
    upstream: Option<&[ExecutionReceipt]>,
    local: Option<&[ExecutionReceipt]>,
) -> Result<usize> {
    let upstream = upstream.context("upstream execution receipts were not requested")?;
    let local = local.context("local execution receipts are unavailable")?;
    if local != upstream {
        bail!(
            "block {height} local trigger, VM state, GAS, exception presence, receipt count, or receipt order differs from both upstreams"
        );
    }
    Ok(local.len())
}

fn decode_block(raw: &[u8], height: u32) -> Result<Block> {
    let mut reader = MemoryReader::new(raw);
    let block = <Block as Serializable>::deserialize(&mut reader)
        .context("strict block deserialization failed")?;
    if reader.remaining() != 0 || block.index() != height {
        bail!("block has trailing bytes or a mismatched height");
    }
    Ok(block)
}

fn parse_hash(value: Option<&str>, field: &str) -> Result<UInt256> {
    let value = value.with_context(|| format!("{field} is not a string"))?;
    UInt256::parse(value).map_err(|error| anyhow::anyhow!("invalid {field}: {error}"))
}

fn strict_u32(value: &Value, field: &str) -> Result<u32> {
    let value = value
        .as_u64()
        .with_context(|| format!("{field} is not an unsigned integer"))?;
    u32::try_from(value).with_context(|| format!("{field} exceeds u32"))
}

fn strict_i64(value: &Value, field: &str) -> Result<i64> {
    if let Some(number) = value.as_i64() {
        return Ok(number);
    }
    value
        .as_str()
        .with_context(|| format!("{field} is not an integer or decimal string"))?
        .parse()
        .with_context(|| format!("{field} is not a signed 64-bit integer"))
}

fn parse_application_log(value: &Value, expected_hash: UInt256) -> Result<Vec<ExecutionReceipt>> {
    let block_hash = parse_hash(
        value.get("blockhash").and_then(Value::as_str),
        "application-log block hash",
    )?;
    if block_hash != expected_hash {
        bail!("application log belongs to another block");
    }
    parse_execution_receipts(value)
}

fn parse_transaction_log(value: &Value, expected_hash: UInt256) -> Result<Vec<ExecutionReceipt>> {
    let transaction_hash = parse_hash(
        value.get("txid").and_then(Value::as_str),
        "application-log transaction hash",
    )?;
    if transaction_hash != expected_hash {
        bail!("application log belongs to another transaction");
    }
    parse_execution_receipts(value)
}

fn parse_execution_receipts(value: &Value) -> Result<Vec<ExecutionReceipt>> {
    value
        .get("executions")
        .and_then(Value::as_array)
        .context("application log executions missing")?
        .iter()
        .map(|execution| {
            let has_exception = match execution.get("exception") {
                None | Some(Value::Null) => false,
                Some(Value::String(message)) => !message.is_empty(),
                Some(_) => bail!("execution exception is not a string or null"),
            };
            Ok(ExecutionReceipt {
                trigger: execution
                    .get("trigger")
                    .and_then(Value::as_str)
                    .context("execution trigger missing")?
                    .to_owned(),
                vm_state: execution
                    .get("vmstate")
                    .and_then(Value::as_str)
                    .context("execution vmstate missing")?
                    .to_owned(),
                gas_consumed: strict_i64(
                    execution
                        .get("gasconsumed")
                        .context("execution gas missing")?,
                    "gas consumed",
                )?,
                has_exception,
            })
        })
        .collect()
}

fn parse_batch_response(
    payload: Value,
    expected: &ExpectedRequests,
) -> Result<HashMap<String, Value>> {
    let responses = payload
        .as_array()
        .context("RPC batch response is not an array")?;
    if responses.len() != expected.len() {
        bail!("RPC batch response count differs from request count");
    }
    let mut results = HashMap::with_capacity(expected.len());
    for response in responses {
        let object = response
            .as_object()
            .context("RPC response is not an object")?;
        if object.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
            bail!("RPC response has an invalid JSON-RPC version");
        }
        let id = object
            .get("id")
            .and_then(Value::as_u64)
            .context("RPC response id is not an unsigned integer")?;
        let (key, method) = expected
            .get(&id)
            .context("RPC response has an unknown id")?;
        if results.contains_key(key) {
            bail!("RPC response contains a duplicate id");
        }
        if let Some(error) = object.get("error").filter(|error| !error.is_null()) {
            bail!("upstream {method} failed: {error}");
        }
        let result = object
            .get("result")
            .cloned()
            .with_context(|| format!("upstream {method} omitted result"))?;
        results.insert(key.clone(), result);
    }
    Ok(results)
}

fn validate_audit_resume_mode(
    file: &mut File,
    checkpoint: Option<&neo_validator::state::VerifiedPoint>,
    verify_receipts: bool,
    new_audit_at_checkpoint: bool,
) -> Result<()> {
    if !new_audit_at_checkpoint {
        return validate_audit_resume(file, checkpoint, verify_receipts);
    }
    if checkpoint.is_none() {
        bail!("--new-audit-at-checkpoint requires an existing validator checkpoint");
    }
    if file.metadata()?.len() != 0 {
        bail!("--new-audit-at-checkpoint requires a missing or empty audit file");
    }
    Ok(())
}

fn validate_audit_resume(
    file: &mut File,
    checkpoint: Option<&neo_validator::state::VerifiedPoint>,
    verify_receipts: bool,
) -> Result<()> {
    let length = file.metadata()?.len();
    let Some(checkpoint) = checkpoint else {
        if length != 0 {
            bail!("a non-empty audit file cannot be paired with a new validator store");
        }
        return Ok(());
    };
    if length == 0 {
        bail!("validator store has a checkpoint but its audit file is empty");
    }
    let tail_length = length.min(8192);
    file.seek(SeekFrom::End(-(tail_length as i64)))?;
    let mut tail = String::new();
    file.read_to_string(&mut tail)?;
    let line = tail
        .lines()
        .rfind(|line| !line.trim().is_empty())
        .context("audit file has no final record")?;
    let record: AuditRecord = serde_json::from_str(line).context("invalid final audit record")?;
    if record.height != checkpoint.height
        || record.block_hash != checkpoint.block_hash.to_string()
        || record.state_root != checkpoint.state_root.to_string()
    {
        bail!("audit watermark does not match the validator store checkpoint");
    }
    if record.receipts_verified != verify_receipts {
        bail!("audit receipt-verification scope differs from the requested scope");
    }
    file.seek(SeekFrom::End(0))?;
    Ok(())
}

fn write_mismatch(path: &Path, audit: &mut impl Write, incident: &Value) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, serde_json::to_vec_pretty(incident)?)?;
    audit.flush()?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_durability_is_explicit_and_defaults_to_per_block_sync() {
        let default_args = Args::try_parse_from(["mainnet-state-verifier"]).unwrap();
        assert!(!default_args.batch_durable);
        let batch_args =
            Args::try_parse_from(["mainnet-state-verifier", "--batch-durable"]).unwrap();
        assert!(batch_args.batch_durable);
    }

    #[test]
    fn cache_ab_and_fresh_audit_are_explicit_opt_ins() {
        let default_args = Args::try_parse_from(["mainnet-state-verifier"]).unwrap();
        assert!(!default_args.disable_mpt_node_cache);
        assert!(!default_args.new_audit_at_checkpoint);
        let experiment = Args::try_parse_from([
            "mainnet-state-verifier",
            "--disable-mpt-node-cache",
            "--new-audit-at-checkpoint",
        ])
        .unwrap();
        assert!(experiment.disable_mpt_node_cache);
        assert!(experiment.new_audit_at_checkpoint);
    }

    #[test]
    fn process_memory_monitor_observes_current_process() {
        let mut monitor = ProcessMemoryMonitor::new().expect("current-process monitor");
        monitor.sample().expect("second RSS sample");
        let profile = monitor.profile();
        assert!(profile.initial_rss_bytes > 0);
        assert!(profile.final_rss_bytes > 0);
        assert!(profile.peak_rss_bytes >= profile.initial_rss_bytes);
        assert!(profile.peak_rss_bytes >= profile.final_rss_bytes);
        assert_eq!(profile.samples, 2);
    }

    #[test]
    fn batch_parser_accepts_out_of_order_responses() {
        let expected = HashMap::from([
            (1, ("block".to_owned(), "getblock" as &'static str)),
            (2, ("root".to_owned(), "getstateroot" as &'static str)),
        ]);
        let payload = json!([
            {"jsonrpc":"2.0", "id":2, "result":{"index":0}},
            {"jsonrpc":"2.0", "id":1, "result":"block"}
        ]);
        let values = parse_batch_response(payload, &expected).expect("valid batch");
        assert_eq!(values.get("block").and_then(Value::as_str), Some("block"));
        assert_eq!(
            values
                .get("root")
                .and_then(|root| root.get("index"))
                .and_then(Value::as_u64),
            Some(0)
        );
    }

    #[test]
    fn batch_parser_rejects_missing_duplicate_unknown_and_rpc_errors() {
        let expected = HashMap::from([(1, ("one".to_owned(), "getblock" as &'static str))]);
        for payload in [
            json!([]),
            json!([{"jsonrpc":"2.0", "id":2, "result":null}]),
            json!([{"jsonrpc":"2.0", "id":1, "error":{"code":-1,"message":"bad"}}]),
        ] {
            assert!(parse_batch_response(payload, &expected).is_err());
        }
        let expected = HashMap::from([(1, ("one".to_owned(), "getblock" as &'static str))]);
        let duplicate = json!([
            {"jsonrpc":"2.0", "id":1, "result":1},
            {"jsonrpc":"2.0", "id":1, "result":2}
        ]);
        assert!(parse_batch_response(duplicate, &expected).is_err());
    }

    #[test]
    fn audit_resume_requires_the_exact_store_watermark() {
        let hash =
            UInt256::parse("0x1f4d1defa46faa5e7b9b8d3f79a06bec777d7c26c4aa5f6f5899a291daa87c15")
                .expect("hash");
        let root =
            UInt256::parse("0x58a5157b7e99eeabf631291f1747ec8eb12ab89461cda888492b17a301de81e8")
                .expect("root");
        let checkpoint = neo_validator::state::VerifiedPoint {
            height: 7,
            block_hash: hash,
            state_root: root,
        };
        let record = AuditRecord {
            height: 7,
            block_hash: hash.to_string(),
            state_root: root.to_string(),
            transaction_count: 2,
            receipts_verified: true,
            receipt_count: 4,
            offline_block_offset: None,
            offline_next_offset: None,
        };
        let mut file = tempfile::NamedTempFile::new().expect("audit file");
        serde_json::to_writer(file.as_file_mut(), &record).expect("write audit");
        file.as_file_mut().write_all(b"\n").expect("write newline");
        file.as_file_mut().flush().expect("flush audit");
        validate_audit_resume(file.as_file_mut(), Some(&checkpoint), true)
            .expect("matching resume");
        assert!(validate_audit_resume(file.as_file_mut(), Some(&checkpoint), false).is_err());
        assert!(validate_audit_resume(file.as_file_mut(), None, true).is_err());

        let mut wrong_checkpoint = checkpoint.clone();
        wrong_checkpoint.height = 8;
        assert!(validate_audit_resume(file.as_file_mut(), Some(&wrong_checkpoint), true).is_err());
        assert!(
            validate_audit_resume_mode(file.as_file_mut(), Some(&checkpoint), true, true).is_err()
        );
        let mut empty = tempfile::NamedTempFile::new().expect("empty audit");
        assert!(validate_audit_resume_mode(empty.as_file_mut(), None, true, true).is_err());
        validate_audit_resume_mode(empty.as_file_mut(), Some(&checkpoint), true, true)
            .expect("explicit fresh audit at a verified checkpoint");
        assert!(validate_audit_resume(empty.as_file_mut(), Some(&wrong_checkpoint), true).is_err());
    }

    fn sample_receipts() -> Vec<ExecutionReceipt> {
        vec![
            ExecutionReceipt {
                trigger: "OnPersist".to_owned(),
                vm_state: "HALT".to_owned(),
                gas_consumed: 0,
                has_exception: false,
            },
            ExecutionReceipt {
                trigger: "Application".to_owned(),
                vm_state: "FAULT".to_owned(),
                gas_consumed: 123_456,
                has_exception: true,
            },
        ]
    }

    #[test]
    fn application_log_parser_is_strict_and_preserves_receipt_fields() {
        let hash =
            UInt256::parse("0x1f4d1defa46faa5e7b9b8d3f79a06bec777d7c26c4aa5f6f5899a291daa87c15")
                .expect("hash");
        let log = json!({
            "blockhash": hash.to_string(),
            "executions": [
                {"trigger":"OnPersist", "vmstate":"HALT", "gasconsumed":"0", "exception":null},
                {"trigger":"Application", "vmstate":"FAULT", "gasconsumed":123456, "exception":"failed"}
            ]
        });
        assert_eq!(
            parse_application_log(&log, hash).expect("valid application log"),
            sample_receipts()
        );
        let mut wrong_hash = log.clone();
        wrong_hash["blockhash"] = json!(UInt256::zero().to_string());
        assert!(parse_application_log(&wrong_hash, hash).is_err());
        assert!(parse_application_log(&json!({"blockhash":hash.to_string()}), hash).is_err());
        let mut invalid_exception = log.clone();
        invalid_exception["executions"][0]["exception"] = json!({"message":"failed"});
        assert!(parse_application_log(&invalid_exception, hash).is_err());
        let mut invalid_gas = log;
        invalid_gas["executions"][0]["gasconsumed"] = json!("not-a-number");
        assert!(parse_application_log(&invalid_gas, hash).is_err());
    }

    #[test]
    fn receipt_comparison_rejects_field_order_count_and_availability_differences() {
        let expected = sample_receipts();
        assert_eq!(
            compare_execution_receipts(7, Some(&expected), Some(&expected))
                .expect("identical receipts"),
            2
        );
        for index in 0..expected.len() {
            for field in 0..4 {
                let mut different = expected.clone();
                match field {
                    0 => different[index].trigger.push_str("Changed"),
                    1 => different[index].vm_state = "NONE".to_owned(),
                    2 => different[index].gas_consumed += 1,
                    _ => different[index].has_exception = !different[index].has_exception,
                }
                assert!(compare_execution_receipts(7, Some(&expected), Some(&different)).is_err());
            }
        }
        let mut reversed = expected.clone();
        reversed.reverse();
        assert!(compare_execution_receipts(7, Some(&expected), Some(&reversed)).is_err());
        assert!(compare_execution_receipts(7, Some(&expected), Some(&expected[..1])).is_err());
        assert!(compare_execution_receipts(7, None, Some(&expected)).is_err());
        assert!(compare_execution_receipts(7, Some(&expected), None).is_err());
    }

    #[test]
    fn upstream_receipt_disagreement_and_missing_logs_are_rejected() {
        let hash = UInt256::zero();
        let make_evidence = |executions| UpstreamEvidence {
            height: 7,
            raw_block: vec![1, 2, 3],
            state_root: hash,
            executions,
        };
        let primary = make_evidence(Some(sample_receipts()));
        let secondary = make_evidence(Some(sample_receipts()));
        assert!(compare_evidence(primary, secondary).is_ok());
        let primary = make_evidence(Some(sample_receipts()));
        let mut different = sample_receipts();
        different[1].gas_consumed += 1;
        let secondary = make_evidence(Some(different));
        assert!(compare_evidence(primary, secondary).is_err());
        let primary = make_evidence(Some(sample_receipts()));
        let secondary = make_evidence(None);
        assert!(compare_evidence(primary, secondary).is_err());
    }

    fn write_test_package(bytes: &[u8]) -> tempfile::NamedTempFile {
        let mut file = tempfile::NamedTempFile::new().expect("offline package");
        file.as_file_mut().write_all(bytes).expect("write package");
        file.as_file_mut().flush().expect("flush package");
        file
    }

    fn genesis_package() -> (tempfile::NamedTempFile, Vec<u8>, u64) {
        let raw = general_purpose::STANDARD.decode("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACI6hnvVQEAAB2sK3wAAAAAAAAAAABrEj3YvscYZIhSu8eFleNTagWPnwEAAREA").expect("genesis Base64");
        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&(raw.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&raw);
        let next_offset = bytes.len() as u64;
        (write_test_package(&bytes), raw, next_offset)
    }

    #[test]
    fn offline_package_reader_matches_neo_cli_layout_and_resume_offsets() {
        let (package, raw, next_offset) = genesis_package();
        let mut source = OfflineBlockSource::open(package.path()).expect("open package");
        assert_eq!(source.start, 0);
        assert_eq!(source.end, 0);
        let batch = source.read_batch(0, 0).expect("read genesis");
        assert_eq!(batch.len(), 1);
        assert_eq!(batch[0].raw, raw);
        assert_eq!(batch[0].block_offset, 8);
        assert_eq!(batch[0].next_offset, next_offset);
        assert!(source.read_one().is_err());

        let mut source = OfflineBlockSource::open(package.path()).expect("reopen package");
        let checkpoint = neo_validator::state::VerifiedPoint {
            height: 0,
            block_hash: decode_block(&raw, 0).expect("genesis").hash(),
            state_root: UInt256::zero(),
        };
        let record = AuditRecord {
            height: 0,
            block_hash: checkpoint.block_hash.to_string(),
            state_root: checkpoint.state_root.to_string(),
            transaction_count: 0,
            receipts_verified: false,
            receipt_count: 0,
            offline_block_offset: Some(8),
            offline_next_offset: Some(next_offset),
        };
        source.resume(&checkpoint, &record).expect("exact resume");
        let mut wrong_record = record;
        wrong_record.offline_next_offset = Some(next_offset + 1);
        assert!(source.resume(&checkpoint, &wrong_record).is_err());
    }

    #[test]
    fn offline_package_reader_rejects_bad_headers_truncation_and_trailing_bytes() {
        let (package, raw, _) = genesis_package();
        let mut bytes = std::fs::read(package.path()).expect("package bytes");
        bytes[0] = 1;
        let wrong_start = write_test_package(&bytes);
        assert!(OfflineBlockSource::open(wrong_start.path()).is_err());

        let mut bytes = std::fs::read(package.path()).expect("package bytes");
        bytes[4] = 2;
        let missing_block = write_test_package(&bytes);
        let mut source = OfflineBlockSource::open(missing_block.path()).expect("open count");
        assert!(source.read_batch(0, 1).is_err());

        let mut bytes = std::fs::read(package.path()).expect("package bytes");
        bytes.truncate(bytes.len() - 1);
        let truncated = write_test_package(&bytes);
        let mut source = OfflineBlockSource::open(truncated.path()).expect("open truncated");
        assert!(source.read_one().is_err());

        let mut bytes = std::fs::read(package.path()).expect("package bytes");
        bytes.push(0);
        let trailing = write_test_package(&bytes);
        let mut source = OfflineBlockSource::open(trailing.path()).expect("open trailing");
        assert!(source.read_one().is_err());

        let mut bytes = Vec::new();
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        bytes.extend_from_slice(&1_u32.to_le_bytes());
        bytes.extend_from_slice(&0_u32.to_le_bytes());
        let empty_block = write_test_package(&bytes);
        let mut source = OfflineBlockSource::open(empty_block.path()).expect("open empty");
        assert!(source.read_one().is_err());
        assert!(!raw.is_empty());
    }

    #[test]
    fn genesis_raw_block_is_strictly_decoded_and_hashed() {
        let raw = general_purpose::STANDARD.decode("AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAACI6hnvVQEAAB2sK3wAAAAAAAAAAABrEj3YvscYZIhSu8eFleNTagWPnwEAAREA").expect("genesis Base64");
        let block = decode_block(&raw, 0).expect("genesis block");
        assert_eq!(
            block.hash().to_string(),
            "0x1f4d1defa46faa5e7b9b8d3f79a06bec777d7c26c4aa5f6f5899a291daa87c15"
        );
        assert!(decode_block(&raw, 1).is_err());
        let mut trailing = raw;
        trailing.push(0);
        assert!(decode_block(&trailing, 0).is_err());
    }
}
