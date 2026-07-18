//! Durable append-frame and sorted-run persistence prototype.

use anyhow::Result;
use clap::Parser;
use neo_benches::append_benchmark::{AppendBenchmarkConfig, run_append_benchmark};
use neo_benches::mdbx_benchmark::{BenchmarkLabels, CampaignScale, SmokeSettings};
use neo_benches::storage_workload::MAINNET_H1_877_001_TO_H1_887_000;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "append-persistence-bench",
    about = "Run a durable append-frame plus immutable sorted-run campaign"
)]
struct Cli {
    /// Fresh prototype directory; nonempty directories are rejected.
    #[arg(long)]
    database: PathBuf,
    /// Atomically published JSON report.
    #[arg(long)]
    output: PathBuf,
    /// Optional JSONL phase checkpoints.
    #[arg(long)]
    evidence_log: Option<PathBuf>,
    /// Exact full corpus or bounded ratio-preserving projection.
    #[arg(long, value_enum, default_value_t = CampaignScale::Smoke)]
    scale: CampaignScale,
    #[arg(long)]
    hardware_profile: String,
    #[arg(long)]
    filesystem_profile: String,
    #[arg(long, default_value = "append-sync-data")]
    durability_profile: String,
    #[arg(long, default_value = "warm-after-prefill")]
    read_cache_state: String,
    #[arg(long, default_value_t = 32_768)]
    prefill_batch_entries: usize,
    #[arg(long, default_value_t = 8_192)]
    point_queries: usize,
    #[arg(long, default_value_t = 3)]
    point_rounds: u32,
    #[arg(long, default_value_t = 4_096)]
    sorted_batch_keys: usize,
    #[arg(long, default_value_t = 3)]
    sorted_batch_rounds: u32,
    #[arg(long, default_value_t = 32_768)]
    smoke_prefill_rows: u64,
    #[arg(long, default_value_t = 8_192)]
    smoke_operations: u64,
    #[arg(long, default_value_t = 100)]
    smoke_blocks: u64,
    /// Hard bound for decoded sorted-run entries retained by the prototype.
    #[arg(long, default_value_t = 1_024)]
    max_index_memory_mib: u64,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let report = run_append_benchmark(&AppendBenchmarkConfig {
        database: cli.database,
        output: cli.output,
        evidence_log: cli.evidence_log,
        shape: MAINNET_H1_877_001_TO_H1_887_000,
        scale: cli.scale,
        smoke: SmokeSettings {
            prefill_rows: cli.smoke_prefill_rows,
            operations: cli.smoke_operations,
            blocks: cli.smoke_blocks,
        },
        prefill_batch_entries: cli.prefill_batch_entries,
        point_queries: cli.point_queries,
        point_rounds: cli.point_rounds,
        sorted_batch_keys: cli.sorted_batch_keys,
        sorted_batch_rounds: cli.sorted_batch_rounds,
        max_index_memory_bytes: cli.max_index_memory_mib.saturating_mul(1024 * 1024),
        labels: BenchmarkLabels {
            hardware: cli.hardware_profile,
            filesystem: cli.filesystem_profile,
            durability: cli.durability_profile,
            read_cache_state: cli.read_cache_state,
        },
    })?;
    println!(
        "wrote append persistence report to {} (campaign_entries={}, campaign_wall_ns={})",
        report.output.display(),
        report.workload.puts + report.workload.tombstones,
        report.campaign.wall_ns
    );
    Ok(())
}
