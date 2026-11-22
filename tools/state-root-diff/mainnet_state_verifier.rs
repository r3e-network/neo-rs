//! Resumable, persistent, two-upstream MainNet block and state-root verifier.

use anyhow::{Context, Result, bail};
use base64::{Engine as _, engine::general_purpose};
use clap::{Parser, ValueEnum};
use neo_io::{MemoryReader, Serializable};
use neo_primitives::{BlockLike, UInt256};
use neo_protocol::ProtocolSettings;
use neo_protocol::payloads::Block;
use neo_system::ExecutionMode;
use neo_validator::executor::{BlockExecutor, NeoExecutor};
use neo_validator::upstream::{
    ExecutionReceipt, parse_execution, parse_hash, strict_u32, validate_protocol_identity,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};
use sysinfo::{Pid, ProcessExt, System, SystemExt};

use config::{Args, RpcEndpoint};
use offline::repair_offline_audit;
use offline_run::run_offline;
use profile::{ExecutionStageProfile, MptNodeCacheProfile, ReplayProfile};
use verify::{validate_audit_resume_mode, validate_identity, verify_range};

type ExpectedRequests = HashMap<u64, (String, &'static str)>;

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
        .map(parse_execution)
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

#[path = "mainnet_state_verifier/config.rs"]
mod config;
#[path = "mainnet_state_verifier/offline.rs"]
mod offline;
#[path = "mainnet_state_verifier/offline_run.rs"]
mod offline_run;
#[path = "mainnet_state_verifier/profile.rs"]
mod profile;
#[path = "mainnet_state_verifier/verify.rs"]
mod verify;

#[cfg(test)]
#[path = "mainnet_state_verifier/tests.rs"]
mod tests;
