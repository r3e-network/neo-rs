use std::{collections::BTreeMap, fs, path::Path, str::FromStr, time::Duration};

use anyhow::{bail, ensure, Context, Result};
use chrono::Utc;
use clap::{Parser, Subcommand};
use neo_base::hash::Hash160;
use neo_consensus::ChangeViewReason;
use neo_node::{NodeStatus, StageState, StageStatus, ValidatorDescriptor};
use neo_wallet::SignerScopes;
use serde::{Deserialize, Serialize};
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
    Wallet {
        #[command(subcommand)]
        command: WalletCommands,
    },
}

#[derive(Subcommand, Debug)]
enum WalletCommands {
    /// List wallet account script hashes known by the node
    Accounts,
    /// List pending transaction identifiers (demo data)
    Pending,
    /// List wallet accounts with metadata
    AccountsDetail {
        /// Wallet password used to decrypt stored entries
        password: String,
    },
    /// Export the WIF private key for an account
    ExportWif {
        /// Script hash of the account to export (hex string, e.g. 0x...)
        script_hash: String,
        /// Wallet password used to decrypt the keystore entry
        password: String,
    },
    /// Export the NEP-2 key for an account
    ExportNep2 {
        /// Script hash of the account to export (hex string, e.g. 0x...)
        script_hash: String,
        /// Wallet password used to decrypt the keystore entry
        password: String,
        /// Passphrase to encrypt the NEP-2 key
        passphrase: String,
        /// Override scrypt N parameter (default 16384)
        #[arg(long)]
        n: Option<u64>,
        /// Override scrypt r parameter (default 8)
        #[arg(long)]
        r: Option<u32>,
        /// Override scrypt p parameter (default 8)
        #[arg(long)]
        p: Option<u32>,
        /// Override address version byte (default 0x35)
        #[arg(long)]
        address_version: Option<u8>,
    },
    /// Import an account from a WIF-encoded private key
    ImportWif {
        /// Base58Check-encoded WIF string
        wif: String,
        /// Wallet password used to encrypt the keystore entry
        password: String,
        /// Mark the imported account as default
        #[arg(long, default_value_t = false)]
        make_default: bool,
    },
    /// Import an account from a NEP-2 encrypted key
    ImportNep2 {
        /// NEP-2 encrypted key string
        nep2: String,
        /// Passphrase used to decrypt the NEP-2 key
        passphrase: String,
        /// Wallet password used to encrypt the keystore entry
        password: String,
        /// Mark the imported account as default
        #[arg(long, default_value_t = false)]
        make_default: bool,
        /// Override scrypt N parameter (default 16384)
        #[arg(long)]
        n: Option<u64>,
        /// Override scrypt r parameter (default 8)
        #[arg(long)]
        r: Option<u32>,
        /// Override scrypt p parameter (default 8)
        #[arg(long)]
        p: Option<u32>,
        /// Override address version byte (default 0x35)
        #[arg(long)]
        address_version: Option<u8>,
    },
    /// Update signer scopes/permissions for an account
    SetSigner {
        /// Script hash of the account to update (hex string, e.g. 0x...)
        script_hash: String,
        /// Wallet password used to decrypt the keystore entry
        password: String,
        /// Witness scopes (e.g. `CalledByEntry|CustomContracts`, `Global`)
        scopes: String,
        /// Allowed contract script hashes (repeat flag)
        #[arg(long = "contract")]
        allowed_contracts: Vec<String>,
        /// Allowed group public keys in hex (repeat flag)
        #[arg(long = "group")]
        allowed_groups: Vec<String>,
    },
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
            WalletCommands::AccountsDetail { password } => {
                let details =
                    fetch_wallet_accounts_detail(&cli.endpoint, &password).await?;
                render_wallet_account_details(&details);
            }
            WalletCommands::ExportWif {
                script_hash,
                password,
            } => {
                let wif = export_wallet_wif(&cli.endpoint, &script_hash, &password).await?;
                println!("WIF: {wif}");
            }
            WalletCommands::ExportNep2 {
                script_hash,
                password,
                passphrase,
                n,
                r,
                p,
                address_version,
            } => {
                let nep2 = export_wallet_nep2(
                    &cli.endpoint,
                    &script_hash,
                    &password,
                    &passphrase,
                    n,
                    r,
                    p,
                    address_version,
                )
                .await?;
                println!("NEP-2: {nep2}");
            }
            WalletCommands::ImportWif {
                wif,
                password,
                make_default,
            } => {
                let hash = import_wallet_wif(&cli.endpoint, &wif, &password, make_default).await?;
                println!("Imported account {}", hash);
            }
            WalletCommands::ImportNep2 {
                nep2,
                passphrase,
                password,
                make_default,
                n,
                r,
                p,
                address_version,
            } => {
                let hash = import_wallet_nep2(
                    &cli.endpoint,
                    &nep2,
                    &passphrase,
                    &password,
                    make_default,
                    n,
                    r,
                    p,
                    address_version,
                )
                .await?;
                println!("Imported account {}", hash);
            }
            WalletCommands::SetSigner {
                script_hash,
                password,
                scopes,
                allowed_contracts,
                allowed_groups,
            } => {
                set_wallet_signer(
                    &cli.endpoint,
                    &script_hash,
                    &password,
                    &scopes,
                    &allowed_contracts,
                    &allowed_groups,
                )
                .await?;
                println!("Updated signer metadata for {}", script_hash);
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

    if status.consensus_change_view_reason_counts.is_empty() {
        println!("Change-view cnts: <none>");
    } else {
        println!("Change-view cnts:");
        for (reason, count) in &status.consensus_change_view_reason_counts {
            println!("  {:<16} {}", reason, count);
        }
    }
    println!(
        "Change-view total: {}",
        status.consensus_change_view_total
    );

    let status_change_view = format_change_view_reasons(&status.consensus_change_view_reasons);
    if status_change_view.is_empty() {
        println!("Change-view     : <none>");
    } else {
        println!("Change-view:");
        for line in status_change_view {
            println!("  {line}");
        }
    }

    if status.consensus_missing.is_empty() {
        println!("Missing         : <none>");
    } else {
        println!("Missing:");
        for (kind, validators) in &status.consensus_missing {
            println!("  {:<16} [{}]", kind, format_id_list(validators));
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
    let url = format!("{}/wallet/accounts", endpoint.trim_end_matches('/'));
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
    let url = format!("{}/wallet/pending", endpoint.trim_end_matches('/'));
    let response = timeout(Duration::from_secs(3), reqwest::get(url)).await??;
    Ok(response.error_for_status()?.json().await?)
}

#[derive(Serialize)]
struct ImportWifPayload<'a> {
    wif: &'a str,
    password: &'a str,
    make_default: bool,
}

async fn import_wallet_wif(
    endpoint: &str,
    wif: &str,
    password: &str,
    make_default: bool,
) -> Result<Hash160> {
    let url = format!("{}/wallet/import/wif", endpoint.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let response = timeout(
        Duration::from_secs(3),
        client
            .post(url)
            .json(&ImportWifPayload {
                wif,
                password,
                make_default,
            })
            .send(),
    )
    .await??;
    Ok(response.error_for_status()?.json().await?)
}

#[derive(Serialize)]
struct ImportNep2Payload<'a> {
    nep2: &'a str,
    passphrase: &'a str,
    password: &'a str,
    make_default: bool,
    n: Option<u64>,
    r: Option<u32>,
    p: Option<u32>,
    address_version: Option<u8>,
}

async fn import_wallet_nep2(
    endpoint: &str,
    nep2: &str,
    passphrase: &str,
    password: &str,
    make_default: bool,
    n: Option<u64>,
    r: Option<u32>,
    p: Option<u32>,
    address_version: Option<u8>,
) -> Result<Hash160> {
    let url = format!("{}/wallet/import/nep2", endpoint.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let response = timeout(
        Duration::from_secs(3),
        client
            .post(url)
            .json(&ImportNep2Payload {
                nep2,
                passphrase,
                password,
                make_default,
                n,
                r,
                p,
                address_version,
            })
            .send(),
    )
    .await??;
    Ok(response.error_for_status()?.json().await?)
}

#[derive(Serialize)]
struct ExportWifPayload<'a> {
    script_hash: &'a str,
    password: &'a str,
}

async fn export_wallet_wif(endpoint: &str, script_hash: &str, password: &str) -> Result<String> {
    let url = format!("{}/wallet/export/wif", endpoint.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let response = timeout(
        Duration::from_secs(3),
        client
            .post(url)
            .json(&ExportWifPayload {
                script_hash,
                password,
            })
            .send(),
    )
    .await??;
    Ok(response.error_for_status()?.json().await?)
}

#[derive(Serialize)]
struct ExportNep2Payload<'a> {
    script_hash: &'a str,
    password: &'a str,
    passphrase: &'a str,
    n: Option<u64>,
    r: Option<u32>,
    p: Option<u32>,
    address_version: Option<u8>,
}

async fn export_wallet_nep2(
    endpoint: &str,
    script_hash: &str,
    password: &str,
    passphrase: &str,
    n: Option<u64>,
    r: Option<u32>,
    p: Option<u32>,
    address_version: Option<u8>,
) -> Result<String> {
    let url = format!("{}/wallet/export/nep2", endpoint.trim_end_matches('/'));
    let client = reqwest::Client::new();
    let response = timeout(
        Duration::from_secs(3),
        client
            .post(url)
            .json(&ExportNep2Payload {
                script_hash,
                password,
                passphrase,
                n,
                r,
                p,
                address_version,
            })
            .send(),
    )
    .await??;
    Ok(response.error_for_status()?.json().await?)
}

#[derive(Deserialize)]
struct AccountDetailModel {
    script_hash: String,
    label: Option<String>,
    is_default: bool,
    lock: bool,
    scopes: String,
    allowed_contracts: Vec<String>,
    allowed_groups: Vec<String>,
}

#[derive(Serialize)]
struct AccountsDetailRequest<'a> {
    password: &'a str,
}

async fn fetch_wallet_accounts_detail(
    endpoint: &str,
    password: &str,
) -> Result<Vec<AccountDetailModel>> {
    let url = format!(
        "{}/wallet/accounts/detail",
        endpoint.trim_end_matches('/')
    );
    let client = reqwest::Client::new();
    let response = timeout(
        Duration::from_secs(3),
        client
            .post(url)
            .json(&AccountsDetailRequest { password })
            .send(),
    )
    .await??;
    Ok(response.error_for_status()?.json().await?)
}

fn render_wallet_account_details(accounts: &[AccountDetailModel]) {
    if accounts.is_empty() {
        println!("No accounts stored.");
        return;
    }
    for detail in accounts {
        println!("{}", detail.script_hash);
        if let Some(label) = &detail.label {
            println!("  Label     : {label}");
        }
        println!("  Default   : {}", detail.is_default);
        println!("  Locked    : {}", detail.lock);
        println!("  Scopes    : {}", detail.scopes);
        if !detail.allowed_contracts.is_empty() {
            println!("  Contracts : {:?}", detail.allowed_contracts);
        }
        if !detail.allowed_groups.is_empty() {
            println!("  Groups    : {:?}", detail.allowed_groups);
        }
        println!();
    }
}

#[derive(Serialize)]
struct UpdateSignerPayload {
    script_hash: String,
    password: String,
    scopes: String,
    allowed_contracts: Vec<String>,
    allowed_groups: Vec<String>,
}

async fn set_wallet_signer(
    endpoint: &str,
    script_hash: &str,
    password: &str,
    scopes: &str,
    allowed_contracts: &[String],
    allowed_groups: &[String],
) -> Result<()> {
    let parsed_hash = Hash160::from_str(script_hash)
        .with_context(|| format!("invalid script hash '{script_hash}'"))?;
    let mut parsed_scopes = SignerScopes::from_witness_scope_string(scopes)
        .ok_or_else(|| anyhow::anyhow!(format!("invalid scopes '{scopes}'")))?
        .clone();
    if parsed_scopes.is_empty() {
        parsed_scopes = SignerScopes::CALLED_BY_ENTRY;
    }
    if parsed_scopes.contains(SignerScopes::WITNESS_RULES) {
        bail!("WitnessRules scope is not supported yet");
    }
    if !parsed_scopes.is_valid() {
        bail!("invalid scope combination: {}", parsed_scopes.to_witness_scope_string());
    }
    if parsed_scopes.contains(SignerScopes::GLOBAL)
        && (!allowed_contracts.is_empty() || !allowed_groups.is_empty())
    {
        bail!("Global scope cannot be combined with allowed contracts or groups");
    }

    let normalized_contracts = allowed_contracts
        .iter()
        .map(|value| {
            Hash160::from_str(value)
                .map(|hash| hash.to_string())
                .with_context(|| format!("invalid allowed contract '{value}'"))
        })
        .collect::<Result<Vec<_>>>()?;
    if parsed_scopes.contains(SignerScopes::CUSTOM_CONTRACTS) {
        ensure!(
            !normalized_contracts.is_empty(),
            "CustomContracts scope requires at least one allowed contract"
        );
    } else {
        ensure!(
            normalized_contracts.is_empty(),
            "Allowed contracts require the CustomContracts scope"
        );
    }

    let normalized_groups = allowed_groups
        .iter()
        .map(|value| {
            let trimmed = value.trim();
            let trimmed = trimmed.strip_prefix("0x").unwrap_or(trimmed);
            let bytes = hex::decode(trimmed)
                .with_context(|| format!("invalid allowed group '{value}'"))?;
            ensure!(
                bytes.len() == 33,
                "allowed groups must be 33-byte compressed public keys"
            );
            Ok(format!("0x{}", hex::encode(bytes)))
        })
        .collect::<Result<Vec<_>>>()?;
    if parsed_scopes.contains(SignerScopes::CUSTOM_GROUPS) {
        ensure!(
            !normalized_groups.is_empty(),
            "CustomGroups scope requires at least one allowed group"
        );
    } else {
        ensure!(
            normalized_groups.is_empty(),
            "Allowed groups require the CustomGroups scope"
        );
    }

    let payload = UpdateSignerPayload {
        script_hash: parsed_hash.to_string(),
        password: password.to_owned(),
        scopes: parsed_scopes.to_witness_scope_string(),
        allowed_contracts: normalized_contracts,
        allowed_groups: normalized_groups,
    };

    let url = format!(
        "{}/wallet/update/signer",
        endpoint.trim_end_matches('/')
    );
    let client = reqwest::Client::new();
    let response = timeout(
        Duration::from_secs(3),
        client.post(url).json(&payload).send(),
    )
    .await??;
    response.error_for_status()?;
    Ok(())
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
        let status = stages.get(*stage).cloned().unwrap_or_else(|| StageStatus {
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

fn render_validators(label: &str, validators: &[ValidatorDescriptor], primary: Option<u16>) {
    if validators.is_empty() {
        println!("{:<16}: <none>", label);
        return;
    }
    println!("{label}:");
    println!(
        "  {:<4} {:<18} {:<20} {}",
        "ID", "Alias", "Script hash", "Public key"
    );
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

fn format_change_view_reasons(
    reasons: &BTreeMap<u16, ChangeViewReason>,
) -> Vec<String> {
    reasons
        .iter()
        .map(|(validator, reason)| format!("validator {:<3} -> {}", validator, reason))
        .collect()
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
        assert_eq!(format_state_label(&StageState::Complete, false), "complete");
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

    #[test]
    fn format_change_view_reasons_outputs_lines() {
        let mut reasons = BTreeMap::new();
        reasons.insert(3, ChangeViewReason::Manual);
        reasons.insert(1, ChangeViewReason::Timeout);
        let lines = format_change_view_reasons(&reasons);
        assert_eq!(
            lines,
            vec![
                "validator 1   -> Timeout".to_string(),
                "validator 3   -> Manual".to_string()
            ]
        );
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
    #[serde(default)]
    change_view_reasons: BTreeMap<u16, ChangeViewReason>,
    #[serde(default)]
    change_view_reason_counts: BTreeMap<String, usize>,
    #[serde(default)]
    change_view_total: u64,
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

    if status.change_view_reason_counts.is_empty() {
        println!("Change-view cnts : <none>");
    } else {
        println!("Change-view cnts:");
        for (reason, count) in &status.change_view_reason_counts {
            println!("  {:<16} {}", reason, count);
        }
    }
    println!("Change-view total: {}", status.change_view_total);

    let change_view_lines = format_change_view_reasons(&status.change_view_reasons);
    if change_view_lines.is_empty() {
        println!("Change-view      : <none>");
    } else {
        println!("Change-view:");
        for line in change_view_lines {
            println!("  {line}");
        }
    }

    if status.missing.is_empty() {
        println!("Missing          : <none>");
    } else {
        println!("Missing:");
        for (kind, validators) in &status.missing {
            println!("  {:<16} [{}]", kind, format_id_list(validators));
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
