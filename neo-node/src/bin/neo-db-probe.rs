//! Offline storage probe for Neo node databases.

use std::{
    collections::VecDeque,
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use clap::{Parser, ValueEnum};
use neo_config::ProtocolSettings;
use neo_execution::{ApplicationEngine, ContractState, Diagnostic, ExecutionContextState};
use neo_io::Serializable;
use neo_manifest::CallFlags;
use neo_native_contracts::{GasToken, StandardNativeProvider};
use neo_payloads::{Block, Transaction, TransactionState};
use neo_primitives::{TriggerType, UInt160, UInt256, Verifiable};
use neo_serialization::BinarySerializer;
use neo_state_service::MptStore;
use neo_storage::persistence::storage::StorageConfig;
use neo_storage::persistence::store::Store;
use neo_storage::persistence::{SeekDirection, StoreCache, StoreFactory};
use neo_storage::{DataCache, StorageKey};
use neo_vm::{ExecutionContext, stack_value_as_bigint};
use neo_vm_rs::{
    ExecutionEngineLimits, Instruction, StackValue, VmState as VMState, stack_value_as_u32,
};
use num_bigint::BigInt;
use parking_lot::Mutex;
use serde_json::{Value, json};

#[derive(Debug, Parser)]
#[command(
    name = "neo-db-probe",
    about = "Read Neo contract storage from an offline database without starting a node"
)]
struct Cli {
    #[arg(long, value_name = "PATH")]
    db: PathBuf,

    #[arg(long, value_enum, default_value_t = default_storage_provider_arg())]
    storage_provider: StorageProviderArg,

    #[arg(long, value_name = "ADDRESS")]
    gas_address: Option<String>,

    #[arg(long, value_name = "HASH")]
    ledger_tx: Option<String>,

    #[arg(long, value_name = "HASH")]
    contract_state: Option<String>,

    #[arg(long, value_name = "HASH")]
    replay_tx: Option<String>,

    #[arg(long, value_name = "BASE64")]
    replay_raw_tx_base64: Option<String>,

    #[arg(long, value_name = "BASE64")]
    replay_block_base64: Option<String>,

    #[arg(long)]
    dump_contract_storage: bool,

    #[arg(long)]
    mpt_state_height: bool,

    #[arg(long, value_name = "HEIGHT")]
    mpt_state_root: Option<u32>,

    #[arg(long, value_name = "HEIGHT")]
    mpt_key_root: Option<u32>,

    #[arg(long, value_name = "HEIGHT")]
    mpt_dump_contract_root: Option<u32>,

    #[arg(long, value_name = "HEIGHT")]
    mpt_dump_root: Option<u32>,

    #[arg(long, default_value_t = 200)]
    dump_limit: usize,

    #[arg(long, value_name = "ID", allow_hyphen_values = true)]
    contract_id: Option<i32>,

    #[arg(long, value_name = "BASE64")]
    key_base64: Option<String>,

    #[arg(long, value_name = "HEX")]
    key_hex: Option<String>,

    #[arg(long, value_enum, default_value_t = DecodeMode::Hex)]
    decode: DecodeMode,

    #[arg(long, value_name = "BASE64")]
    write_value_base64: Option<String>,
}

#[derive(Clone, Copy, Debug, ValueEnum)]
enum DecodeMode {
    Hex,
    Base64,
    RawBigint,
    NeoAccount,
    Nep17Account,
    HashIndex,
    TransactionState,
    ContractState,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, ValueEnum)]
enum StorageProviderArg {
    Mdbx,
    Rocksdb,
}

impl StorageProviderArg {
    fn as_provider_name(self) -> &'static str {
        match self {
            StorageProviderArg::Mdbx => "mdbx",
            StorageProviderArg::Rocksdb => "rocksdb",
        }
    }
}

fn default_storage_provider_arg() -> StorageProviderArg {
    StorageProviderArg::Mdbx
}

#[derive(Debug)]
struct ProbeRequest {
    contract_id: i32,
    key_suffix: Vec<u8>,
}

#[derive(Debug, PartialEq, Eq)]
struct HashIndexState {
    hash_hex_le: String,
    index: u32,
}

#[derive(Debug, PartialEq, Eq)]
struct NeoAccountStateProbe {
    balance: BigInt,
    balance_height: u32,
    vote_to_hex: Option<String>,
    last_gas_per_vote: BigInt,
}

#[derive(Debug, Clone)]
struct TraceContextLabel {
    script_hash: Option<String>,
    method: Option<String>,
}

#[derive(Debug)]
struct ReplayInstructionTracer {
    limit: usize,
    events: Arc<Mutex<VecDeque<Value>>>,
    contexts: Vec<TraceContextLabel>,
}

impl ReplayInstructionTracer {
    fn new(limit: usize, events: Arc<Mutex<VecDeque<Value>>>) -> Self {
        Self {
            limit,
            events,
            contexts: Vec::new(),
        }
    }

    fn push_event(&self, event: Value) {
        let mut events = self.events.lock();
        if events.len() == self.limit {
            events.pop_front();
        }
        events.push_back(event);
    }

    fn label_context(context: &ExecutionContext) -> TraceContextLabel {
        let state_arc =
            context.get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
        let state = state_arc.lock();
        TraceContextLabel {
            script_hash: state
                .script_hash
                .or_else(|| UInt160::from_bytes(&context.script_hash()).ok())
                .map(|hash| hash.to_string()),
            method: state.method_name.clone(),
        }
    }
}

impl Diagnostic for ReplayInstructionTracer {
    fn initialized(&mut self, _engine: &mut ApplicationEngine) {}

    fn disposed(&mut self) {}

    fn context_loaded(&mut self, context: &ExecutionContext) {
        let label = Self::label_context(context);
        self.contexts.push(label.clone());
        self.push_event(json!({
            "event": "context_loaded",
            "depth": self.contexts.len(),
            "script_hash": label.script_hash,
            "method": label.method,
        }));
    }

    fn context_unloaded(&mut self, context: &ExecutionContext) {
        let label = Self::label_context(context);
        self.push_event(json!({
            "event": "context_unloaded",
            "depth": self.contexts.len(),
            "script_hash": label.script_hash,
            "method": label.method,
        }));
        self.contexts.pop();
    }

    fn pre_execute_instruction(&mut self, instruction: &Instruction) {
        let label = self.contexts.last().cloned();
        self.push_event(json!({
            "event": "pre",
            "depth": self.contexts.len(),
            "script_hash": label.as_ref().and_then(|label| label.script_hash.clone()),
            "method": label.and_then(|label| label.method),
            "ip": instruction.pointer,
            "opcode": format!("{:?}", instruction.opcode()),
            "operand_hex": hex::encode(instruction.operand()),
        }));
    }

    fn post_execute_instruction(&mut self, instruction: &Instruction) {
        let label = self.contexts.last().cloned();
        self.push_event(json!({
            "event": "post",
            "depth": self.contexts.len(),
            "script_hash": label.as_ref().and_then(|label| label.script_hash.clone()),
            "method": label.and_then(|label| label.method),
            "ip": instruction.pointer,
            "opcode": format!("{:?}", instruction.opcode()),
        }));
    }
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    if cli.mpt_state_height
        || cli.mpt_state_root.is_some()
        || cli.mpt_key_root.is_some()
        || cli.mpt_dump_contract_root.is_some()
        || cli.mpt_dump_root.is_some()
    {
        ensure_mpt_probe_args(&cli)?;
        let output = probe_mpt_state(
            cli.storage_provider,
            &cli.db,
            cli.mpt_state_height,
            cli.mpt_state_root,
            cli.mpt_key_root,
            cli.mpt_dump_contract_root,
            cli.mpt_dump_root,
            cli.contract_id,
            cli.key_base64.as_deref(),
            cli.key_hex.as_deref(),
            cli.dump_limit,
            cli.decode,
        )
        .with_context(|| format!("read StateService MPT store {}", cli.db.display()))?;
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if let Some(tx_hash) = cli.replay_tx.as_deref() {
        ensure!(
            cli.gas_address.is_none()
                && cli.ledger_tx.is_none()
                && cli.contract_state.is_none()
                && cli.replay_raw_tx_base64.is_none()
                && cli.replay_block_base64.is_none()
                && cli.contract_id.is_none()
                && cli.key_base64.is_none()
                && cli.key_hex.is_none(),
            "--replay-tx cannot be combined with storage probe arguments"
        );
        let output = replay_transaction(cli.storage_provider, &cli.db, tx_hash)
            .with_context(|| format!("replay transaction {tx_hash} from {}", cli.db.display()))?;
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if let Some(raw_tx) = cli.replay_raw_tx_base64.as_deref() {
        ensure!(
            cli.gas_address.is_none()
                && cli.ledger_tx.is_none()
                && cli.contract_state.is_none()
                && cli.replay_tx.is_none()
                && cli.contract_id.is_none()
                && cli.key_base64.is_none()
                && cli.key_hex.is_none()
                && cli.write_value_base64.is_none(),
            "--replay-raw-tx-base64 cannot be combined with storage probe arguments"
        );
        let block = cli.replay_block_base64.as_deref().ok_or_else(|| {
            anyhow!("--replay-block-base64 is required with --replay-raw-tx-base64")
        })?;
        let output = replay_raw_transaction(cli.storage_provider, &cli.db, raw_tx, block)
            .with_context(|| format!("replay raw transaction against {}", cli.db.display()))?;
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if cli.dump_contract_storage {
        ensure!(
            cli.gas_address.is_none()
                && cli.ledger_tx.is_none()
                && cli.contract_state.is_none()
                && cli.replay_tx.is_none()
                && cli.replay_raw_tx_base64.is_none()
                && cli.replay_block_base64.is_none()
                && cli.key_base64.is_none()
                && cli.key_hex.is_none()
                && cli.write_value_base64.is_none(),
            "--dump-contract-storage can only be combined with --contract-id and --dump-limit"
        );
        let contract_id = cli
            .contract_id
            .ok_or_else(|| anyhow!("--contract-id is required with --dump-contract-storage"))?;
        let output =
            dump_contract_storage(cli.storage_provider, &cli.db, contract_id, cli.dump_limit)
                .with_context(|| format!("dump contract storage from {}", cli.db.display()))?;
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    let request = build_probe_request(&cli)?;
    let raw_key = storage_key_bytes(request.contract_id, &request.key_suffix);
    let written_len = if let Some(value) = cli.write_value_base64.as_deref() {
        let value = base64_decode(value).context("decode --write-value-base64")?;
        write_storage_value(
            cli.storage_provider,
            &cli.db,
            request.contract_id,
            request.key_suffix.clone(),
            value.clone(),
        )
        .with_context(|| {
            format!(
                "write {} at {}",
                cli.storage_provider.as_provider_name(),
                cli.db.display()
            )
        })?;
        Some(value.len())
    } else {
        None
    };
    let value = read_storage_value(
        cli.storage_provider,
        &cli.db,
        request.contract_id,
        request.key_suffix.clone(),
    )
    .with_context(|| {
        format!(
            "read {} at {}",
            cli.storage_provider.as_provider_name(),
            cli.db.display()
        )
    })?;

    let mut output = json!({
        "db": cli.db,
        "storage_provider": cli.storage_provider.as_provider_name(),
        "contract_id": request.contract_id,
        "key_base64": base64_encode(&request.key_suffix),
        "key_hex": hex::encode(&request.key_suffix),
        "storage_key_hex": hex::encode(raw_key),
        "found": value.is_some(),
    });
    if let Some(written_len) = written_len {
        output["written_value_len"] = json!(written_len);
    }

    if let Some(value) = value {
        output["value_base64"] = json!(base64_encode(&value));
        output["value_hex"] = json!(hex::encode(&value));
        output["decoded"] = decode_value(cli.decode, &value)?;
    }

    println!("{}", serde_json::to_string_pretty(&output)?);
    Ok(())
}

fn ensure_mpt_probe_args(cli: &Cli) -> Result<()> {
    if cli.mpt_key_root.is_some() {
        ensure!(
            cli.contract_id.is_some(),
            "--contract-id is required with --mpt-key-root"
        );
        ensure!(
            cli.key_base64.is_some() ^ cli.key_hex.is_some(),
            "use exactly one of --key-base64 or --key-hex with --mpt-key-root"
        );
    } else if cli.mpt_dump_contract_root.is_some() {
        ensure!(
            cli.contract_id.is_some(),
            "--contract-id is required with --mpt-dump-contract-root"
        );
        ensure!(
            cli.key_base64.is_none() && cli.key_hex.is_none(),
            "--mpt-dump-contract-root cannot be combined with --key-base64 or --key-hex"
        );
    } else if cli.mpt_dump_root.is_some() {
        ensure!(
            cli.contract_id.is_none() && cli.key_base64.is_none() && cli.key_hex.is_none(),
            "--mpt-dump-root cannot be combined with chain storage probe arguments"
        );
    } else {
        ensure!(
            cli.contract_id.is_none() && cli.key_base64.is_none() && cli.key_hex.is_none(),
            "--mpt-state-height/--mpt-state-root cannot be combined with chain storage probe arguments"
        );
    }
    ensure!(
        cli.gas_address.is_none()
            && cli.ledger_tx.is_none()
            && cli.contract_state.is_none()
            && cli.replay_tx.is_none()
            && cli.replay_raw_tx_base64.is_none()
            && cli.replay_block_base64.is_none()
            && !cli.dump_contract_storage
            && cli.write_value_base64.is_none(),
        "--mpt-state-height/--mpt-state-root cannot be combined with chain storage probe arguments"
    );
    Ok(())
}

fn build_probe_request(cli: &Cli) -> Result<ProbeRequest> {
    if let Some(address) = cli.gas_address.as_deref() {
        ensure!(
            cli.ledger_tx.is_none()
                && cli.contract_state.is_none()
                && cli.contract_id.is_none()
                && cli.key_base64.is_none()
                && cli.key_hex.is_none(),
            "--gas-address cannot be combined with --ledger-tx, --contract-state, --contract-id, --key-base64, or --key-hex"
        );
        return Ok(ProbeRequest {
            contract_id: GasToken::ID,
            key_suffix: gas_account_key_from_address(address)?,
        });
    }

    if let Some(hash) = cli.ledger_tx.as_deref() {
        ensure!(
            cli.contract_state.is_none()
                && cli.contract_id.is_none()
                && cli.key_base64.is_none()
                && cli.key_hex.is_none(),
            "--ledger-tx cannot be combined with --contract-state, --contract-id, --key-base64, or --key-hex"
        );
        return Ok(ProbeRequest {
            contract_id: -4,
            key_suffix: ledger_transaction_key_from_hash(hash)?,
        });
    }

    if let Some(hash) = cli.contract_state.as_deref() {
        ensure!(
            cli.contract_id.is_none() && cli.key_base64.is_none() && cli.key_hex.is_none(),
            "--contract-state cannot be combined with --contract-id, --key-base64, or --key-hex"
        );
        return Ok(ProbeRequest {
            contract_id: -1,
            key_suffix: contract_state_key_from_hash(hash)?,
        });
    }

    let contract_id = cli.contract_id.ok_or_else(|| {
        anyhow!(
            "--contract-id is required unless --gas-address, --ledger-tx, or --contract-state is used"
        )
    })?;
    let key_suffix = match (cli.key_base64.as_deref(), cli.key_hex.as_deref()) {
        (Some(_), Some(_)) => bail!("use either --key-base64 or --key-hex, not both"),
        (Some(encoded), None) => base64_decode(encoded).context("decode --key-base64")?,
        (None, Some(encoded)) => {
            hex::decode(encoded).with_context(|| format!("decode --key-hex {encoded}"))?
        }
        (None, None) => bail!("--key-base64 or --key-hex is required unless --gas-address is used"),
    };

    Ok(ProbeRequest {
        contract_id,
        key_suffix,
    })
}

fn gas_account_key_from_address(address: &str) -> Result<Vec<u8>> {
    let script_hash =
        UInt160::from_address(address).map_err(|err| anyhow!("invalid Neo address: {err}"))?;
    let mut key = Vec::with_capacity(1 + 20);
    key.push(0x14);
    key.extend_from_slice(&script_hash.to_array());
    Ok(key)
}

fn ledger_transaction_key_from_hash(hash: &str) -> Result<Vec<u8>> {
    let hash = UInt256::from_str(hash).map_err(|err| anyhow!("invalid transaction hash: {err}"))?;
    let mut key = Vec::with_capacity(1 + 32);
    key.push(0x0b);
    key.extend_from_slice(&hash.to_array());
    Ok(key)
}

fn contract_state_key_from_hash(hash: &str) -> Result<Vec<u8>> {
    let hash = UInt160::from_str(hash).map_err(|err| anyhow!("invalid contract hash: {err}"))?;
    let mut key = Vec::with_capacity(1 + 20);
    key.push(0x08);
    key.extend_from_slice(&hash.to_array());
    Ok(key)
}

fn storage_key_bytes(contract_id: i32, key_suffix: &[u8]) -> Vec<u8> {
    StorageKey::new(contract_id, key_suffix.to_vec()).to_array()
}

fn storage_key_prefix_bytes(contract_id: i32) -> Vec<u8> {
    StorageKey::new(contract_id, Vec::new()).to_array()
}

fn mpt_state_root_key(index: u32) -> Vec<u8> {
    let mut key = vec![0x01];
    key.extend_from_slice(&index.to_be_bytes());
    key
}

fn mpt_current_local_root_index_key() -> Vec<u8> {
    vec![0x02]
}

fn probe_mpt_state(
    storage_provider: StorageProviderArg,
    db_path: &Path,
    include_height: bool,
    root_index: Option<u32>,
    key_root_index: Option<u32>,
    dump_contract_root_index: Option<u32>,
    dump_root_index: Option<u32>,
    contract_id: Option<i32>,
    key_base64: Option<&str>,
    key_hex: Option<&str>,
    dump_limit: usize,
    decode: DecodeMode,
) -> Result<Value> {
    let store = open_store(storage_provider, db_path, true)?;
    let snapshot = store.snapshot();
    let mut output = json!({
        "db": db_path,
        "storage_provider": storage_provider.as_provider_name(),
        "mode": "state-service-mpt",
    });

    if include_height {
        let key = mpt_current_local_root_index_key();
        let value = snapshot.try_get(&key);
        let mut height = json!({
            "key_hex": hex::encode(&key),
            "found": value.is_some(),
        });
        if let Some(bytes) = value {
            height["value_base64"] = json!(base64_encode(&bytes));
            height["value_hex"] = json!(hex::encode(&bytes));
            height["decoded"] = json!({
                "current_local_root_index": decode_mpt_current_local_root_index(&bytes)?,
            });
        }
        output["height"] = height;
    }

    if let Some(index) = root_index {
        let key = mpt_state_root_key(index);
        let value = snapshot.try_get(&key);
        let mut root = json!({
            "index": index,
            "key_hex": hex::encode(&key),
            "found": value.is_some(),
        });
        if let Some(bytes) = value {
            root["value_base64"] = json!(base64_encode(&bytes));
            root["value_hex"] = json!(hex::encode(&bytes));
            root["decoded"] = decode_mpt_state_root_record(&bytes)?;
        }
        output["state_root"] = root;
    }

    if let Some(index) = key_root_index {
        let contract_id = contract_id.ok_or_else(|| anyhow!("--contract-id is required"))?;
        let key_suffix = match (key_base64, key_hex) {
            (Some(_), Some(_)) => bail!("use either --key-base64 or --key-hex, not both"),
            (Some(encoded), None) => base64_decode(encoded).context("decode --key-base64")?,
            (None, Some(encoded)) => {
                hex::decode(encoded).with_context(|| format!("decode --key-hex {encoded}"))?
            }
            (None, None) => bail!("--key-base64 or --key-hex is required with --mpt-key-root"),
        };
        let storage_key = storage_key_bytes(contract_id, &key_suffix);
        let mpt = MptStore::from_store(Arc::clone(&store), true)
            .map_err(|err| anyhow!("open StateService MPT view: {err}"))?;
        let root = mpt
            .get_state_root(index)
            .ok_or_else(|| anyhow!("state root {index} was not found in local MPT store"))?;
        let mut trie = mpt.open_trie(Some(*root.root_hash()));
        let value = trie
            .get(&storage_key)
            .map_err(|err| anyhow!("read MPT key at root {index}: {err}"))?;
        let mut mpt_value = json!({
            "index": index,
            "contract_id": contract_id,
            "key_base64": base64_encode(&key_suffix),
            "key_hex": hex::encode(&key_suffix),
            "storage_key_hex": hex::encode(&storage_key),
            "root_hash": root.root_hash().to_string(),
            "found": value.is_some(),
        });
        if let Some(bytes) = value {
            mpt_value["value_base64"] = json!(base64_encode(&bytes));
            mpt_value["value_hex"] = json!(hex::encode(&bytes));
            mpt_value["decoded"] = decode_value(decode, &bytes)?;
        }
        output["mpt_value"] = mpt_value;
    }

    if let Some(index) = dump_contract_root_index {
        ensure!(dump_limit > 0, "--dump-limit must be greater than zero");
        let contract_id = contract_id.ok_or_else(|| anyhow!("--contract-id is required"))?;
        let mpt = MptStore::from_store(Arc::clone(&store), true)
            .map_err(|err| anyhow!("open StateService MPT view: {err}"))?;
        let root = mpt
            .get_state_root(index)
            .ok_or_else(|| anyhow!("state root {index} was not found in local MPT store"))?;
        let mut trie = mpt.open_trie(Some(*root.root_hash()));
        let prefix = storage_key_prefix_bytes(contract_id);
        let mut entries = trie
            .find_limited(&prefix, None, dump_limit + 1)
            .map_err(|err| anyhow!("dump MPT contract storage at root {index}: {err}"))?;
        let truncated = entries.len() > dump_limit;
        entries.truncate(dump_limit);
        let entries = entries
            .into_iter()
            .map(|entry| {
                let key_suffix = entry.key.get(std::mem::size_of::<i32>()..).unwrap_or(&[]);
                json!({
                    "key_base64": base64_encode(key_suffix),
                    "key_hex": hex::encode(key_suffix),
                    "storage_key_hex": hex::encode(&entry.key),
                    "value_base64": base64_encode(&entry.value),
                    "value_hex": hex::encode(&entry.value),
                })
            })
            .collect::<Vec<_>>();
        output["mpt_contract_storage"] = json!({
            "index": index,
            "contract_id": contract_id,
            "storage_prefix_hex": hex::encode(prefix),
            "root_hash": root.root_hash().to_string(),
            "entry_count": entries.len(),
            "truncated": truncated,
            "entries": entries,
        });
    }

    if let Some(index) = dump_root_index {
        ensure!(dump_limit > 0, "--dump-limit must be greater than zero");
        let mpt = MptStore::from_store(Arc::clone(&store), true)
            .map_err(|err| anyhow!("open StateService MPT view: {err}"))?;
        let root = mpt
            .get_state_root(index)
            .ok_or_else(|| anyhow!("state root {index} was not found in local MPT store"))?;
        let mut trie = mpt.open_trie(Some(*root.root_hash()));
        let mut entries = trie
            .find_limited(&[], None, dump_limit + 1)
            .map_err(|err| anyhow!("dump MPT root {index}: {err}"))?;
        let truncated = entries.len() > dump_limit;
        entries.truncate(dump_limit);
        let mut contract_counts = std::collections::BTreeMap::<i32, usize>::new();
        let entries = entries
            .into_iter()
            .map(|entry| {
                let contract_id = entry
                    .key
                    .get(..std::mem::size_of::<i32>())
                    .and_then(|bytes| bytes.try_into().ok())
                    .map(i32::from_le_bytes);
                if let Some(contract_id) = contract_id {
                    *contract_counts.entry(contract_id).or_default() += 1;
                }
                let key_suffix = entry.key.get(std::mem::size_of::<i32>()..).unwrap_or(&[]);
                json!({
                    "contract_id": contract_id,
                    "key_base64": base64_encode(key_suffix),
                    "key_hex": hex::encode(key_suffix),
                    "storage_key_hex": hex::encode(&entry.key),
                    "value_base64": base64_encode(&entry.value),
                    "value_hex": hex::encode(&entry.value),
                })
            })
            .collect::<Vec<_>>();
        output["mpt_root_storage"] = json!({
            "index": index,
            "root_hash": root.root_hash().to_string(),
            "entry_count": entries.len(),
            "truncated": truncated,
            "contract_counts": contract_counts,
            "entries": entries,
        });
    }

    Ok(output)
}

fn dump_contract_storage(
    storage_provider: StorageProviderArg,
    db_path: &Path,
    contract_id: i32,
    limit: usize,
) -> Result<Value> {
    ensure!(limit > 0, "--dump-limit must be greater than zero");
    let store = open_store(storage_provider, db_path, true)?;
    let prefix = StorageKey::new(contract_id, Vec::new());
    let mut entries = Vec::new();
    let mut truncated = false;

    for (key, item) in store.find(Some(&prefix), SeekDirection::Forward) {
        if entries.len() >= limit {
            truncated = true;
            break;
        }
        let value = item.to_value();
        entries.push(json!({
            "key_base64": base64_encode(key.key()),
            "key_hex": hex::encode(key.key()),
            "storage_key_hex": hex::encode(key.to_array()),
            "value_base64": base64_encode(&value),
            "value_hex": hex::encode(&value),
        }));
    }

    Ok(json!({
        "db": db_path,
        "storage_provider": storage_provider.as_provider_name(),
        "contract_id": contract_id,
        "storage_prefix_hex": hex::encode(storage_key_prefix_bytes(contract_id)),
        "entry_count": entries.len(),
        "truncated": truncated,
        "entries": entries,
    }))
}

fn read_storage_value(
    storage_provider: StorageProviderArg,
    db_path: &Path,
    contract_id: i32,
    key_suffix: Vec<u8>,
) -> Result<Option<Vec<u8>>> {
    let store = open_store(storage_provider, db_path, true)?;
    let key = StorageKey::new(contract_id, key_suffix);
    Ok(store.try_get(&key).map(|item| item.to_value()))
}

fn write_storage_value(
    storage_provider: StorageProviderArg,
    db_path: &Path,
    contract_id: i32,
    key_suffix: Vec<u8>,
    value: Vec<u8>,
) -> Result<()> {
    let store = open_store(storage_provider, db_path, false)?;
    let mut snapshot = store.snapshot();
    let snapshot = Arc::get_mut(&mut snapshot).ok_or_else(|| {
        anyhow!(
            "{} snapshot is unexpectedly shared",
            storage_provider.as_provider_name()
        )
    })?;
    snapshot.put_sync(storage_key_bytes(contract_id, &key_suffix), value)?;
    snapshot.try_commit()?;
    Ok(())
}

fn open_store(
    storage_provider: StorageProviderArg,
    db_path: &Path,
    read_only: bool,
) -> Result<Arc<dyn Store>> {
    StoreFactory::get_store_with_config(
        storage_provider.as_provider_name(),
        StorageConfig {
            path: db_path.to_path_buf(),
            read_only,
            ..StorageConfig::default()
        },
    )
    .map_err(|err| anyhow!("open {} store: {err}", storage_provider.as_provider_name()))
}

fn open_store_cache(storage_provider: StorageProviderArg, db_path: &Path) -> Result<StoreCache> {
    let store = open_store(storage_provider, db_path, true)?;
    Ok(StoreCache::new_from_store(store, false))
}

fn replay_transaction(
    storage_provider: StorageProviderArg,
    db_path: &Path,
    tx_hash: &str,
) -> Result<Value> {
    let tx_hash =
        UInt256::from_str(tx_hash).map_err(|err| anyhow!("invalid transaction hash: {err}"))?;
    let trace_instruction_limit = std::env::var("NEO_DB_PROBE_TRACE_INSTRUCTIONS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|limit| *limit > 0);
    let trace_events = trace_instruction_limit
        .map(|limit| Arc::new(Mutex::new(VecDeque::with_capacity(limit.saturating_add(1)))));
    let diagnostic = trace_instruction_limit.zip(trace_events.as_ref()).map(
        |(limit, events)| -> Box<dyn Diagnostic> {
            Box::new(ReplayInstructionTracer::new(limit, Arc::clone(events)))
        },
    );

    let store_cache = open_store_cache(storage_provider, db_path)?;
    let ledger = neo_native_contracts::LedgerContract::new();
    let snapshot = store_cache.data_cache();
    let tx_state = ledger
        .get_transaction_state(snapshot, &tx_hash)?
        .ok_or_else(|| anyhow!("transaction {tx_hash} was not found in Ledger storage"))?;
    let block_index = tx_state.block_index;
    let block_hash = ledger
        .get_block_hash(snapshot, block_index)?
        .ok_or_else(|| anyhow!("block hash for height {block_index} was not found"))?;
    let trimmed = ledger
        .get_trimmed_block(snapshot, &block_hash)?
        .ok_or_else(|| anyhow!("trimmed block {block_hash} was not found"))?;

    let mut transactions = Vec::with_capacity(trimmed.hashes.len());
    for hash in &trimmed.hashes {
        let state = ledger
            .get_transaction_state(snapshot, hash)?
            .ok_or_else(|| anyhow!("transaction state {hash} referenced by block is missing"))?;
        let transaction = state
            .transaction
            .ok_or_else(|| anyhow!("transaction state {hash} is a conflict stub"))?;
        transactions.push(transaction);
    }

    let block = Arc::new(Block {
        header: trimmed.header,
        transactions,
    });
    let transaction = block
        .transactions
        .iter()
        .find(|tx| tx.try_hash().ok().as_ref() == Some(&tx_hash))
        .cloned()
        .ok_or_else(|| anyhow!("transaction {tx_hash} was not found in block {block_index}"))?;

    execute_transaction_probe(
        db_path,
        snapshot,
        block,
        transaction,
        Some(tx_state.state),
        diagnostic,
        trace_events,
    )
}

fn replay_raw_transaction(
    storage_provider: StorageProviderArg,
    db_path: &Path,
    raw_tx_base64: &str,
    block_base64: &str,
) -> Result<Value> {
    let trace_instruction_limit = std::env::var("NEO_DB_PROBE_TRACE_INSTRUCTIONS")
        .ok()
        .and_then(|raw| raw.parse::<usize>().ok())
        .filter(|limit| *limit > 0);
    let trace_events = trace_instruction_limit
        .map(|limit| Arc::new(Mutex::new(VecDeque::with_capacity(limit.saturating_add(1)))));
    let diagnostic = trace_instruction_limit.zip(trace_events.as_ref()).map(
        |(limit, events)| -> Box<dyn Diagnostic> {
            Box::new(ReplayInstructionTracer::new(limit, Arc::clone(events)))
        },
    );

    let transaction = decode_raw_transaction(raw_tx_base64)?;
    let tx_hash = transaction.try_hash()?;
    let block = Arc::new(decode_raw_block(block_base64)?);
    ensure!(
        block
            .transactions
            .iter()
            .any(|tx| tx.try_hash().ok().as_ref() == Some(&tx_hash)),
        "raw transaction {tx_hash} is not included in supplied block"
    );

    let store_cache = open_store_cache(storage_provider, db_path)?;
    let snapshot = store_cache.data_cache();
    execute_transaction_probe(
        db_path,
        snapshot,
        block,
        transaction,
        None,
        diagnostic,
        trace_events,
    )
}

fn decode_raw_transaction(raw_tx_base64: &str) -> Result<Transaction> {
    let bytes = base64_decode(raw_tx_base64).context("decode raw transaction base64")?;
    let mut reader = neo_io::MemoryReader::new(&bytes);
    let transaction = <Transaction as Serializable>::deserialize(&mut reader)
        .map_err(|err| anyhow!("decode raw transaction: {err}"))?;
    ensure!(
        reader.remaining() == 0,
        "raw transaction has {} trailing byte(s)",
        reader.remaining()
    );
    Ok(transaction)
}

fn decode_raw_block(block_base64: &str) -> Result<Block> {
    let bytes = base64_decode(block_base64).context("decode raw block base64")?;
    let mut reader = neo_io::MemoryReader::new(&bytes);
    let block = <Block as Serializable>::deserialize(&mut reader)
        .map_err(|err| anyhow!("decode raw block: {err}"))?;
    ensure!(
        reader.remaining() == 0,
        "raw block has {} trailing byte(s)",
        reader.remaining()
    );
    Ok(block)
}

fn execute_transaction_probe(
    db_path: &Path,
    snapshot: &DataCache,
    block: Arc<Block>,
    transaction: Transaction,
    stored_vm_state: Option<VMState>,
    diagnostic: Option<Box<dyn Diagnostic>>,
    trace_events: Option<Arc<Mutex<VecDeque<Value>>>>,
) -> Result<Value> {
    let tx_hash = transaction.try_hash()?;
    let block_hash = block.hash();
    let block_index = block.header.index();

    let block_cache = Arc::new(snapshot.clone_cache());
    let tx_cache = Arc::new(block_cache.clone_cache());
    let container: Arc<dyn Verifiable> = Arc::new(transaction.clone());
    let mut engine = ApplicationEngine::new_with_shared_block_and_native_contract_provider(
        TriggerType::Application,
        Some(container),
        Arc::clone(&tx_cache),
        Some(Arc::clone(&block)),
        ProtocolSettings::mainnet(),
        transaction.system_fee(),
        diagnostic,
        Some(Arc::new(StandardNativeProvider::new())),
    )?;
    let (vm_state, load_error) =
        match engine.load_script(transaction.script().to_vec(), CallFlags::ALL, None) {
            Ok(()) => (engine.execute_allow_fault(), None),
            Err(error) => (VMState::FAULT, Some(error.to_string())),
        };
    let exception = engine.fault_exception().map(str::to_owned).or(load_error);
    let notification_summary = engine
        .notifications()
        .iter()
        .map(|notification| {
            json!({
                "contract": notification.script_hash.to_string(),
                "event": notification.event_name,
                "state_len": notification.state.len(),
            })
        })
        .collect::<Vec<_>>();

    let instruction_trace = trace_events
        .as_ref()
        .map(|events| events.lock().iter().cloned().collect::<Vec<Value>>());

    let mut output = json!({
        "db": db_path,
        "tx_hash": tx_hash.to_string(),
        "block_index": block_index,
        "block_hash": block_hash.to_string(),
        "stored_vm_state": stored_vm_state.map(vm_state_name),
        "replayed_vm_state": vm_state_name(vm_state),
        "gas_consumed": engine.fee_consumed().to_string(),
        "exception": exception,
        "current_script_hash": engine.current_script_hash().map(|hash| hash.to_string()),
        "calling_script_hash": engine.get_calling_script_hash().map(|hash| hash.to_string()),
        "entry_script_hash": engine.entry_script_hash().map(|hash| hash.to_string()),
        "invocation_depth": engine.invocation_stack().len(),
        "frames": trace_engine_frames(&engine),
        "notifications": notification_summary,
    });
    if let Some(instruction_trace) = instruction_trace {
        output["instruction_trace"] = json!(instruction_trace);
    }
    Ok(output)
}

fn trace_engine_frames(engine: &ApplicationEngine) -> Vec<Value> {
    engine
        .invocation_stack()
        .iter()
        .enumerate()
        .map(|(index, context)| {
            let state_arc = context
                .get_state_with_factory::<ExecutionContextState, _>(ExecutionContextState::new);
            let state = state_arc.lock();
            let script_hash = state
                .script_hash
                .or_else(|| UInt160::from_bytes(&context.script_hash()).ok())
                .map(|hash| hash.to_string());
            let opcode = context
                .current_instruction()
                .map(|instruction| format!("{:?}", instruction.opcode()))
                .unwrap_or_else(|_| "<none>".to_string());
            json!({
                "index": index,
                "script_hash": script_hash,
                "method": state.method_name,
                "ip": context.instruction_pointer(),
                "opcode": opcode,
            })
        })
        .collect()
}

fn decode_value(mode: DecodeMode, value: &[u8]) -> Result<Value> {
    let decoded = match mode {
        DecodeMode::Hex => json!({
            "format": "hex",
            "value": hex::encode(value),
        }),
        DecodeMode::Base64 => json!({
            "format": "base64",
            "value": base64_encode(value),
        }),
        DecodeMode::RawBigint => json!({
            "format": "raw-bigint",
            "value": decode_raw_bigint(value).to_string(),
        }),
        DecodeMode::NeoAccount => {
            let state = decode_neo_account_state(value)?;
            json!({
                "format": "neo-account",
                "balance": state.balance.to_string(),
                "balance_height": state.balance_height,
                "vote_to": state.vote_to_hex,
                "last_gas_per_vote": state.last_gas_per_vote.to_string(),
            })
        }
        DecodeMode::Nep17Account => json!({
            "format": "nep17-account",
            "balance": decode_nep17_account_balance(value)?.to_string(),
        }),
        DecodeMode::HashIndex => {
            let state = decode_hash_index_state(value)?;
            json!({
                "format": "hash-index",
                "hash_hex_le": state.hash_hex_le,
                "index": state.index,
            })
        }
        DecodeMode::TransactionState => {
            let state = decode_transaction_state(value)?;
            json!({
                "format": "transaction-state",
                "block_index": state.block_index,
                "state": vm_state_name(state.state),
                "state_byte": state.state.to_byte(),
                "has_transaction": state.transaction.is_some(),
                "transaction": state.transaction.as_ref().map(|tx| {
                    json!({
                        "hash": tx.try_hash().map(|hash| hash.to_string()).unwrap_or_else(|err| format!("hash-error: {err}")),
                        "sender": tx.sender().map(|sender| sender.to_string()),
                        "system_fee": tx.system_fee(),
                        "network_fee": tx.network_fee(),
                        "valid_until_block": tx.valid_until_block(),
                    })
                }),
            })
        }
        DecodeMode::ContractState => {
            let state = ContractState::deserialize_contract_record(value)
                .map_err(|err| anyhow!("decode ContractState: {err}"))?;
            json!({
                "format": "contract-state",
                "id": state.id,
                "update_counter": state.update_counter,
                "hash": state.hash.to_string(),
                "compiler": state.nef.compiler,
                "script_len": state.nef.script.len(),
                "script_base64": base64_encode(&state.nef.script),
                "methods": state.manifest.abi.methods.iter().map(|method| {
                    json!({
                        "name": method.name,
                        "param_count": method.parameters.len(),
                        "return_type": format!("{:?}", method.return_type),
                        "offset": method.offset,
                        "safe": method.safe,
                    })
                }).collect::<Vec<_>>(),
            })
        }
    };
    Ok(decoded)
}

fn decode_transaction_state(bytes: &[u8]) -> Result<TransactionState> {
    let value = deserialize_stack_value(bytes).context("deserialize TransactionState")?;
    let mut state = TransactionState::new(0, None, VMState::NONE);
    state
        .from_stack_value(value)
        .map_err(|err| anyhow!("decode TransactionState: {err}"))?;
    Ok(state)
}

fn vm_state_name(state: VMState) -> &'static str {
    match state {
        VMState::NONE => "NONE",
        VMState::HALT => "HALT",
        VMState::FAULT => "FAULT",
        VMState::BREAK => "BREAK",
    }
}

fn decode_hash_index_state(bytes: &[u8]) -> Result<HashIndexState> {
    let value = deserialize_stack_value(bytes).context("deserialize HashIndexState")?;
    let StackValue::Struct(items) = value else {
        bail!("expected HashIndexState struct");
    };
    ensure!(
        items.len() >= 2,
        "HashIndexState struct is shorter than expected"
    );

    let hash_bytes = items[0]
        .to_byte_string_bytes()
        .ok_or_else(|| anyhow!("HashIndexState hash is not byte-like"))?;
    ensure!(
        hash_bytes.len() == 32,
        "HashIndexState hash has {} bytes, expected 32",
        hash_bytes.len()
    );
    let index =
        stack_value_as_u32(&items[1]).ok_or_else(|| anyhow!("HashIndexState index is not u32"))?;

    Ok(HashIndexState {
        hash_hex_le: hex::encode(hash_bytes),
        index,
    })
}

fn decode_neo_account_state(bytes: &[u8]) -> Result<NeoAccountStateProbe> {
    let value = deserialize_stack_value(bytes).context("deserialize NEO account state")?;
    let StackValue::Struct(items) = value else {
        bail!("expected NEO account state struct");
    };
    ensure!(
        items.len() >= 4,
        "NEO account state struct is shorter than expected"
    );

    let balance =
        stack_value_as_bigint(&items[0]).map_err(|err| anyhow!("decode NEO balance: {err}"))?;
    let balance_height = stack_value_as_u32(&items[1])
        .ok_or_else(|| anyhow!("NEO account balance_height is not u32"))?;
    let vote_to_hex = match &items[2] {
        StackValue::Null => None,
        item => {
            let bytes = item
                .to_byte_string_bytes()
                .ok_or_else(|| anyhow!("NEO account vote_to is not byte-like"))?;
            ensure!(
                bytes.len() == 33,
                "NEO account vote_to has {} bytes, expected 33",
                bytes.len()
            );
            Some(hex::encode(bytes))
        }
    };
    let last_gas_per_vote = stack_value_as_bigint(&items[3])
        .map_err(|err| anyhow!("decode NEO last_gas_per_vote: {err}"))?;

    Ok(NeoAccountStateProbe {
        balance,
        balance_height,
        vote_to_hex,
        last_gas_per_vote,
    })
}

fn decode_mpt_current_local_root_index(bytes: &[u8]) -> Result<u32> {
    ensure!(
        bytes.len() == 4,
        "current local root index has {} bytes, expected 4",
        bytes.len()
    );
    let mut arr = [0u8; 4];
    arr.copy_from_slice(bytes);
    Ok(u32::from_le_bytes(arr))
}

fn decode_mpt_state_root_record(bytes: &[u8]) -> Result<Value> {
    const STATE_ROOT_UNSIGNED_LEN: usize = 1 + 4 + 32;
    ensure!(
        bytes.len() >= STATE_ROOT_UNSIGNED_LEN,
        "state-root record has {} bytes, expected at least {}",
        bytes.len(),
        STATE_ROOT_UNSIGNED_LEN
    );
    let version = bytes[0];
    let mut index_bytes = [0u8; 4];
    index_bytes.copy_from_slice(&bytes[1..5]);
    let index = u32::from_le_bytes(index_bytes);
    let root_hash = UInt256::from_bytes(&bytes[5..STATE_ROOT_UNSIGNED_LEN])
        .map_err(|err| anyhow!("invalid state-root hash bytes: {err}"))?;
    Ok(json!({
        "version": version,
        "index": index,
        "roothash": root_hash.to_string(),
        "roothash_hex_le": hex::encode(root_hash.to_array()),
        "trailing_bytes": bytes.len().saturating_sub(STATE_ROOT_UNSIGNED_LEN),
    }))
}

fn decode_nep17_account_balance(bytes: &[u8]) -> Result<BigInt> {
    if bytes.is_empty() {
        return Ok(BigInt::from(0));
    }
    let value = deserialize_stack_value(bytes).context("deserialize NEP-17 account state")?;

    let StackValue::Struct(items) = value else {
        bail!("expected NEP-17 account state struct");
    };
    let balance = items
        .first()
        .ok_or_else(|| anyhow!("NEP-17 account state struct is empty"))?;
    stack_value_as_bigint(balance).map_err(|err| anyhow!("decode NEP-17 balance: {err}"))
}

fn deserialize_stack_value(bytes: &[u8]) -> Result<StackValue> {
    let limits = ExecutionEngineLimits::default();
    BinarySerializer::deserialize_stack_value_with_limits(
        bytes,
        limits.max_item_size as usize,
        limits.max_stack_size as usize,
    )
    .map_err(|err| anyhow!("{err}"))
}

fn decode_raw_bigint(bytes: &[u8]) -> BigInt {
    BigInt::from_signed_bytes_le(bytes)
}

fn base64_encode(bytes: &[u8]) -> String {
    BASE64_STANDARD.encode(bytes)
}

fn base64_decode(value: &str) -> Result<Vec<u8>> {
    BASE64_STANDARD
        .decode(value)
        .map_err(|err| anyhow!("invalid base64: {err}"))
}

#[cfg(test)]
#[path = "../tests/bin/neo_db_probe.rs"]
mod tests;
