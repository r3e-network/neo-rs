use std::{collections::BTreeMap, fs, path::Path, time::Duration};

use anyhow::Result;
use clap::{Parser, Subcommand};
use chrono::Utc;
use neo_base::hash::Hash160;
use neo_node::{NodeStatus, StageState, StageStatus, ValidatorDescriptor};
use serde::Deserialize;
use tokio::time::timeout;

const CONSENSUS_STAGES: &[&str] = &["PrepareRequest", "PrepareResponse", "Commit", "ChangeView"];

#[derive(Parser, Debug)]
#[command(name = "neo", about = "Neo N3 Rust CLI")]
struct Cli {
    /// RPC endpoint exposed by neo-node
    #[arg(long, default_value = "http://127.0.0.1:20332")]
    endpoint: String,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Query node status metrics
    Status {
        /// Override stale threshold in milliseconds (0 disables local highlighting)
        #[arg(long)]
        stale_threshold_ms: Option<u64>,
    },

    /// Query consensus participation details
    Consensus {
        /// Override stale threshold in milliseconds (0 disables local highlighting)
        #[arg(long)]
        stale_threshold_ms: Option<u64>,
        /// Write validator roster as JSON to the given path
        #[arg(long)]
        export_validators: Option<std::path::PathBuf>,
    },

    /// Wallet management operations
    Wallet { #[command(subcommand)] command: WalletCommands },
}

#[derive(Subcommand, Debug)]
enum WalletCommands {
    /// List wallet account script hashes known by the node
    Accounts,
    /// List pending transaction identifiers (demo data)
    Pending,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    match cli.command {
        Commands::Status { stale_threshold_ms } => {
            let status = fetch_status(&cli.endpoint).await?;
            render_status(&status, stale_threshold_ms.map(|v| v as u128));
        }
        Commands::Consensus {
            stale_threshold_ms,
            export_validators,
        } => {
            let status = fetch_consensus_status(&cli.endpoint).await?;
            render_consensus(
                &status,
                stale_threshold_ms.map(|v| v as u128),
                export_validators.as_ref().map(|v| v.as_path()),
            )?;
        }
        Commands::Wallet { command } => match command {
            WalletCommands::Accounts => {
                let accounts = fetch_wallet_accounts(&cli.endpoint).await?;
                render_wallet_accounts(&accounts);
            }
            WalletCommands::Pending => {
                let pending = fetch_wallet_pending(&cli.endpoint).await?;
                render_pending_transactions(&pending);
            }
        },
    }
    Ok(())
}

async fn fetch_status(endpoint: &str) -> Result<NodeStatus> {
    let url = format!("{}/status", endpoint.trim_end_matches('/'));
    let response = timeout(Duration::from_secs(3), reqwest::get(url)).await??;
    Ok(response.error_for_status()?.json().await?)
}

fn render_status(status: &NodeStatus, stale_override: Option<u128>) {
    println!("Network         : {}", status.network);
    println!("Height          : {}", status.height);
    println!("View            : {}", status.view);
    println!("Connected peers : {}", status.connected_peers);
    println!("Timestamp       : {}", status.timestamp);
    println!("Base fee        : {}", status.base_fee);
    println!("Byte fee        : {}", status.byte_fee);
    println!("Mempool size    : {}", status.mempool_size);
    println!("Total tx        : {}", status.total_transactions);
    println!("Total fees      : {}", status.total_fees);
    if status.consensus_quorum > 0 {
        println!("Quorum threshold: {}", status.consensus_quorum);
    }
    if let Some(primary) = status.consensus_primary {
        println!("Primary leader  : {}", primary);
    }
    if let Some(server_threshold) = status.consensus_stale_threshold_ms {
        println!("Stage threshold : {}ms (server)", server_threshold);
    }
    if let Some(ts) = status.consensus_stage_timestamp {
        println!("Stage evaluated : {}", ts);
    }
    if let Some(threshold) = stale_override {
        println!("Stale threshold : {}ms (override)", threshold);
    }
    render_stage_summary(
        &status.consensus_stages,
        stale_override,
        status.consensus_stale_threshold_ms,
    );
    render_validators(
        "Validators",
        &status.consensus_validators,
        status.consensus_primary,
    );

    if status.consensus_participation.is_empty() {
        println!("Participation   : <none>");
    } else {
        println!("Participation:");
        for (kind, validators) in &status.consensus_participation {
            println!("  {:<16} [{}]", kind, format_id_list(validators));
        }
    }

    if status.consensus_tallies.is_empty() {
        println!("Tallies         : <none>");
    } else {
        println!("Tallies:");
        for (kind, count) in &status.consensus_tallies {
            println!("  {:<16} {}", kind, count);
        }
    }

    if status.consensus_missing.is_empty() {
        println!("Missing         : <none>");
    } else {
        println!("Missing:");
        for (kind, validators) in &status.consensus_missing {
            println!(
                "  {:<16} [{}]",
                kind,
                format_id_list(validators)
            );
        }
    }

    if status.consensus_expected.is_empty() {
        println!("Expected       : <none>");
    } else {
        println!("Expected:");
        for (kind, count) in &status.consensus_expected {
            println!("  {:<16} {}", kind, count);
        }
    }
}

async fn fetch_wallet_accounts(endpoint: &str) -> Result<Vec<Hash160>> {
    let url = format!(
        "{}/wallet/accounts",
        endpoint.trim_end_matches('/')
    );
    let response = timeout(Duration::from_secs(3), reqwest::get(url)).await??;
    Ok(response.error_for_status()?.json().await?)
}

fn render_wallet_accounts(accounts: &[Hash160]) {
    if accounts.is_empty() {
        println!("No accounts stored.");
        return;
    }
    println!("Accounts ({}):", accounts.len());
    for hash in accounts {
        println!("- {}", hash);
    }
}

async fn fetch_wallet_pending(endpoint: &str) -> Result<Vec<String>> {
    let url = format!(
        "{}/wallet/pending",
        endpoint.trim_end_matches('/')
    );
    let response = timeout(Duration::from_secs(3), reqwest::get(url)).await??;
    Ok(response.error_for_status()?.json().await?)
}

fn render_pending_transactions(pending: &[String]) {
    if pending.is_empty() {
        println!("No pending transactions queued.");
        return;
    }
    println!("Pending transactions ({}):", pending.len());
    for id in pending {
        println!("- {}", id);
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StageDisplay {
    name: String,
    state: StageState,
    stale: bool,
    expected: Option<usize>,
    responded: usize,
    missing: Vec<u16>,
    age_ms: u128,
}

fn render_stage_summary(
    stages: &BTreeMap<String, StageStatus>,
    override_threshold: Option<u128>,
    server_threshold: Option<u128>,
) {
    if stages.is_empty() {
        println!("Consensus stages: <no data>");
        return;
    }
    println!("Consensus stages:");
    for row in summarize_stages(stages, override_threshold, server_threshold) {
        print_stage_line(
            &row.name,
            &row.state,
            row.stale,
            row.expected,
            row.responded,
            &row.missing,
            row.age_ms,
        );
    }
    for (name, status) in stages {
        if CONSENSUS_STAGES.iter().any(|stage| stage == &name.as_str()) {
            continue;
        }
        let effective_threshold = override_threshold.or(server_threshold);
        let is_stale = stage_is_stale(status, effective_threshold, server_threshold);
        print_stage_line(
            name,
            &status.state,
            is_stale,
            status.expected,
            status.responded,
            &status.missing,
            status.age_ms,
        );
    }
}

fn summarize_stages(
    stages: &BTreeMap<String, StageStatus>,
    override_threshold: Option<u128>,
    server_threshold: Option<u128>,
) -> Vec<StageDisplay> {
    let mut rows = Vec::new();
    let effective_threshold = override_threshold.or(server_threshold);
    for stage in CONSENSUS_STAGES {
        let status = stages
            .get(*stage)
            .cloned()
            .unwrap_or_else(|| StageStatus {
                state: StageState::Inactive,
                expected: None,
                responded: 0,
                missing: Vec::new(),
                last_updated: Utc::now(),
                age_ms: 0,
                stale: false,
            });
        let stale = stage_is_stale(&status, effective_threshold, server_threshold);
        rows.push(StageDisplay {
            name: stage.to_string(),
            state: status.state.clone(),
            stale,
            expected: status.expected,
            responded: status.responded,
            missing: status.missing.clone(),
            age_ms: status.age_ms,
        });
    }
    rows
}

fn print_stage_line(
    name: &str,
    state: &StageState,
    stale: bool,
    expected: Option<usize>,
    responded: usize,
    missing: &[u16],
    age_ms: u128,
) {
    let expected_str = expected
        .map(|v| v.to_string())
        .unwrap_or_else(|| "-".to_string());
    let missing_str = if missing.is_empty() {
        "-".to_string()
    } else {
        format!("[{}]", format_id_list(missing))
    };
    let updated_str = format_age(age_ms);
    let state_label = format_state_label(state, stale);
    println!(
        "  {:<16} state {:<8} expected {:<3} responded {:<2} missing {:<8} updated {}",
        name, state_label, expected_str, responded, missing_str, updated_str
    );
}

fn render_validators(
    label: &str,
    validators: &[ValidatorDescriptor],
    primary: Option<u16>,
) {
    if validators.is_empty() {
        println!("{:<16}: <none>", label);
        return;
    }
    println!("{label}:");
    println!("  {:<4} {:<18} {:<20} {}", "ID", "Alias", "Script hash", "Public key");
    for descriptor in validators {
        println!(
            "  {:<4} {:<18} {:<20} {}",
            format_primary_id(descriptor.id, primary),
            format_alias(descriptor.alias.as_deref()),
            format_script_hash(&descriptor.script_hash),
            format_public_key(&descriptor.public_key)
        );
    }
}

fn format_script_hash(hash: &str) -> String {
    const PREFIX: usize = 8;
    const SUFFIX: usize = 6;
    if hash.len() <= PREFIX + SUFFIX + 1 {
        hash.to_string()
    } else {
        let prefix = &hash[..PREFIX];
        let suffix = &hash[hash.len() - SUFFIX..];
        format!("{prefix}…{suffix}")
    }
}

fn format_public_key(key: &str) -> String {
    const PREFIX: usize = 10;
    const SUFFIX: usize = 6;
    if key.len() <= PREFIX + SUFFIX + 1 {
        key.to_string()
    } else {
        let prefix = &key[..PREFIX];
        let suffix = &key[key.len() - SUFFIX..];
        format!("{prefix}…{suffix}")
    }
}

fn format_alias(alias: Option<&str>) -> String {
    match alias {
        Some(value) if !value.is_empty() => value.to_string(),
        _ => "<unnamed>".to_string(),
    }
}

fn format_primary_id(id: u16, primary: Option<u16>) -> String {
    match primary {
        Some(p) if p == id => format!("*{}", id),
        _ => id.to_string(),
    }
}

fn format_stage_state(state: &StageState) -> &'static str {
    match state {
        StageState::Inactive => "inactive",
        StageState::Pending => "pending",
        StageState::Complete => "complete",
    }
}

fn format_state_label(state: &StageState, stale: bool) -> String {
    if stale {
        format!("{} (stale)", format_stage_state(state))
    } else {
        format_stage_state(state).to_string()
    }
}

fn stage_is_stale(
    status: &StageStatus,
    calculated_threshold: Option<u128>,
    server_threshold: Option<u128>,
) -> bool {
    if let Some(0) = calculated_threshold {
        return false;
    }
    if let Some(threshold) = calculated_threshold {
        return status.age_ms > threshold;
    }
    if let Some(threshold) = server_threshold {
        return status.age_ms > threshold;
    }
    status.stale
}

fn format_id_list(ids: &[u16]) -> String {
    if ids.is_empty() {
        "-".to_string()
    } else {
        ids.iter()
            .map(|id| id.to_string())
            .collect::<Vec<_>>()
            .join(", ")
    }
}

fn format_age(age_ms: u128) -> String {
    const MS_PER_SECOND: u128 = 1_000;
    const MS_PER_MINUTE: u128 = 60 * MS_PER_SECOND;
    const MS_PER_HOUR: u128 = 60 * MS_PER_MINUTE;

    if age_ms >= MS_PER_HOUR {
        format!("{:.1}h", age_ms as f64 / MS_PER_HOUR as f64)
    } else if age_ms >= MS_PER_MINUTE {
        format!("{:.1}m", age_ms as f64 / MS_PER_MINUTE as f64)
    } else if age_ms >= MS_PER_SECOND {
        format!("{:.1}s", age_ms as f64 / MS_PER_SECOND as f64)
    } else {
        format!("{}ms", age_ms)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    use serde_json::Value;
    use std::path::Path;
    use tempfile::tempdir;

    fn sample_stage(age: u128, stale: bool) -> StageStatus {
        StageStatus {
            state: StageState::Pending,
            expected: Some(4),
            responded: 2,
            missing: vec![1, 3],
            last_updated: Utc::now(),
            age_ms: age,
            stale,
        }
    }

    #[test]
    fn stage_override_zero_disables_stale() {
        let status = sample_stage(10_000, true);
        assert!(!stage_is_stale(&status, Some(0), Some(5_000)));
    }

    #[test]
    fn stage_override_beats_server_threshold() {
        let status = sample_stage(3_000, false);
        assert!(!stage_is_stale(&status, Some(5_000), Some(1_000)));
        let status = sample_stage(6_000, false);
        assert!(stage_is_stale(&status, Some(5_000), Some(10_000)));
    }

    #[test]
    fn stage_uses_server_threshold_when_no_override() {
        let status = sample_stage(6_000, false);
        assert!(stage_is_stale(&status, None, Some(5_000)));
        assert!(!stage_is_stale(&status, None, Some(10_000)));
    }

    #[test]
    fn stage_falls_back_to_flag_when_no_thresholds() {
        let status = sample_stage(1_000, true);
        assert!(stage_is_stale(&status, None, None));
        let status = sample_stage(1_000, false);
        assert!(!stage_is_stale(&status, None, None));
    }

    #[test]
    fn format_state_label_marks_stale() {
        assert_eq!(
            format_state_label(&StageState::Pending, true),
            "pending (stale)"
        );
        assert_eq!(
            format_state_label(&StageState::Complete, false),
            "complete"
        );
    }

    #[test]
    fn format_age_human_readable() {
        assert_eq!(format_age(250), "250ms");
        assert_eq!(format_age(1_500), "1.5s");
        assert_eq!(format_age(90_000), "1.5m");
        assert_eq!(format_age(7_200_000), "2.0h");
    }

    #[test]
    fn summarize_stages_reports_prepare_request_missing() {
        let mut stages = BTreeMap::new();
        stages.insert(
            "PrepareRequest".to_string(),
            StageStatus {
                state: StageState::Pending,
                expected: Some(1),
                responded: 0,
                missing: vec![0],
                last_updated: Utc::now(),
                age_ms: 0,
                stale: false,
            },
        );
        let rows = summarize_stages(&stages, None, Some(5_000));
        let row = rows
            .into_iter()
            .find(|row| row.name == "PrepareRequest")
            .expect("prepare request row present");
        assert_eq!(row.expected, Some(1));
        assert_eq!(row.responded, 0);
        assert_eq!(row.missing, vec![0]);
        assert_eq!(row.state, StageState::Pending);
        assert!(!row.stale);
    }

    #[test]
    fn summarize_stages_defaults_to_inactive_for_missing_entries() {
        let rows = summarize_stages(&BTreeMap::new(), None, None);
        let row = rows
            .into_iter()
            .find(|row| row.name == "PrepareRequest")
            .expect("prepare request row present");
        assert_eq!(row.state, StageState::Inactive);
        assert_eq!(row.expected, None);
        assert_eq!(row.responded, 0);
        assert!(row.missing.is_empty());
    }

    #[test]
    fn export_writes_json_and_creates_parent_dirs() {
        let dir = tempdir().unwrap();
        let path = dir.path().join("nested/validators.json");
        let roster = vec![ValidatorDescriptor {
            id: 1,
            public_key: "02deadbeef".to_string(),
            script_hash: "0x1234abcd".to_string(),
            alias: Some("alpha".to_string()),
        }];

        export_validators(&path, &roster).unwrap();

        let raw = std::fs::read_to_string(&path).expect("file exists");
        let parsed: Vec<Value> = serde_json::from_str(&raw).expect("valid json");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0]["alias"], "alpha");
        assert_eq!(parsed[0]["id"], 1);
    }

    #[test]
    fn export_allows_stdout_dash() {
        let roster = Vec::<ValidatorDescriptor>::new();
        export_validators(Path::new("-"), &roster).unwrap();
    }
}

#[derive(Debug, Deserialize)]
struct ConsensusStats {
    height: u64,
    view: u32,
    quorum: usize,
    primary: Option<u16>,
    validators: Vec<ValidatorDescriptor>,
    tallies: BTreeMap<String, usize>,
    participation: BTreeMap<String, Vec<u16>>,
    missing: BTreeMap<String, Vec<u16>>,
    expected: BTreeMap<String, usize>,
    stages: BTreeMap<String, StageStatus>,
    stale_threshold_ms: Option<u128>,
}

async fn fetch_consensus_status(endpoint: &str) -> Result<ConsensusStats> {
    let url = format!("{}/consensus", endpoint.trim_end_matches('/'));
    let response = timeout(Duration::from_secs(3), reqwest::get(url)).await??;
    Ok(response.error_for_status()?.json().await?)
}

fn export_validators(path: &Path, validators: &[ValidatorDescriptor]) -> Result<()> {
    let json = serde_json::to_string_pretty(validators)?;
    if path == Path::new("-") {
        println!("{json}");
    } else {
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                fs::create_dir_all(parent)?;
            }
        }
        fs::write(path, json)?;
        println!("Saved validator roster to {}", path.display());
    }
    Ok(())
}

fn render_consensus(
    status: &ConsensusStats,
    stale_override: Option<u128>,
    export_path: Option<&Path>,
) -> Result<()> {
    println!("Consensus height : {}", status.height);
    println!("Consensus view   : {}", status.view);
    println!("Quorum threshold : {}", status.quorum);
    if let Some(primary) = status.primary {
        println!("Primary leader   : {}", primary);
    }
    if let Some(server_threshold) = status.stale_threshold_ms {
        println!("Stage threshold  : {}ms (server)", server_threshold);
    }
    if let Some(threshold) = stale_override {
        println!("Stale threshold  : {}ms (override)", threshold);
    }
    render_stage_summary(&status.stages, stale_override, status.stale_threshold_ms);
    render_validators("Validators", &status.validators, status.primary);
    if let Some(path) = export_path {
        export_validators(path, &status.validators)?;
    }

    if status.participation.is_empty() {
        println!("Participation    : <none>");
    } else {
        println!("Participation:");
        for (kind, validators) in &status.participation {
            println!("  {:<16} [{}]", kind, format_id_list(validators));
        }
    }

    if status.tallies.is_empty() {
        println!("Tallies          : <none>");
    } else {
        println!("Tallies:");
        for (kind, count) in &status.tallies {
            println!("  {:<16} {}", kind, count);
        }
    }

    if status.missing.is_empty() {
        println!("Missing          : <none>");
    } else {
        println!("Missing:");
        for (kind, validators) in &status.missing {
            println!(
                "  {:<16} [{}]",
                kind,
                format_id_list(validators)
            );
        }
    }

    if status.expected.is_empty() {
        println!("Expected        : <none>");
    } else {
        println!("Expected:");
        for (kind, count) in &status.expected {
            println!("  {:<16} {}", kind, count);
        }
    }

    Ok(())
}
