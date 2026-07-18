//! Durable current-MDBX persistence benchmark runner.

use anyhow::Result;
use clap::Parser;
use neo_benches::mdbx_benchmark::{
    BenchmarkLabels, CampaignScale, MdbxBenchmarkConfig, SmokeSettings, run_mdbx_benchmark,
    validate_benchmark_artifact_paths, write_json_report,
};
use neo_benches::storage_workload::MAINNET_H1_877_001_TO_H1_887_000;
use std::path::PathBuf;

#[derive(Debug, Parser)]
#[command(
    name = "mdbx-persistence-bench",
    about = "Run a durable, prefilled MDBX persistence campaign"
)]
struct Cli {
    /// Fresh database directory; nonempty directories are rejected.
    #[arg(long)]
    database: PathBuf,

    /// Atomically published JSON report.
    #[arg(long)]
    output: PathBuf,

    /// Optional JSONL phase checkpoints for external pidstat/iostat correlation.
    #[arg(long)]
    evidence_log: Option<PathBuf>,

    /// Exact full corpus or bounded ratio-preserving projection.
    #[arg(long, value_enum, default_value_t = CampaignScale::Smoke)]
    scale: CampaignScale,

    /// Required machine/hardware profile label.
    #[arg(long)]
    hardware_profile: String,

    /// Required filesystem/device profile label.
    #[arg(long)]
    filesystem_profile: String,

    /// Explicit durability policy label.
    #[arg(long, default_value = "mdbx-safe-durable")]
    durability_profile: String,

    /// Declared cache state for read percentiles.
    #[arg(long, default_value = "uncontrolled-warm-after-prefill")]
    read_cache_state: String,

    /// Entries per durable prefill transaction.
    #[arg(long, default_value_t = 32_768)]
    prefill_batch_entries: usize,

    /// Present/absent point-query corpus size.
    #[arg(long, default_value_t = 8_192)]
    point_queries: usize,

    /// Point-query repetitions.
    #[arg(long, default_value_t = 3)]
    point_rounds: u32,

    /// Sorted keys per batch lookup.
    #[arg(long, default_value_t = 4_096)]
    sorted_batch_keys: usize,

    /// Sorted-batch repetitions.
    #[arg(long, default_value_t = 3)]
    sorted_batch_rounds: u32,

    /// Smoke-mode prefill rows.
    #[arg(long, default_value_t = 32_768)]
    smoke_prefill_rows: u64,

    /// Smoke-mode timed mutations.
    #[arg(long, default_value_t = 8_192)]
    smoke_operations: u64,

    /// Smoke-mode represented blocks.
    #[arg(long, default_value_t = 100)]
    smoke_blocks: u64,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    validate_benchmark_artifact_paths(&cli.database, cli.evidence_log.as_deref(), &cli.output)?;
    let config = MdbxBenchmarkConfig {
        database: cli.database,
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
        labels: BenchmarkLabels {
            hardware: cli.hardware_profile,
            filesystem: cli.filesystem_profile,
            durability: cli.durability_profile,
            read_cache_state: cli.read_cache_state,
        },
    };
    let report = run_mdbx_benchmark(&config)?;
    write_json_report(&cli.output, &report)?;
    println!(
        "wrote MDBX persistence report to {} (scale={}, campaign_entries={}, campaign_wall_ns={})",
        cli.output.display(),
        report.scale.as_str(),
        report.campaign.logical.entries,
        report.campaign.wall_ns
    );
    Ok(())
}
