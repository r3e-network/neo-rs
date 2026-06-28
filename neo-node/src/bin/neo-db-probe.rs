//! Offline RocksDB storage probe for Neo node databases.

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
use neo_manifest::CallFlags;
use neo_native_contracts::GasToken;
use neo_payloads::{Block, TransactionState};
use neo_primitives::{TriggerType, UInt160, UInt256, Verifiable};
use neo_serialization::BinarySerializer;
use neo_storage::StorageKey;
use neo_storage::persistence::SeekDirection;
use neo_storage::persistence::StoreCache;
use neo_storage::persistence::storage::StorageConfig;
use neo_storage::persistence::store_provider::StoreProvider;
use neo_storage::rocksdb::RocksDBStoreProvider;
use neo_vm::ExecutionContext;
use neo_vm_rs::{
    ExecutionEngineLimits, Instruction, StackValue, VmState as VMState, stack_value_as_bigint,
    stack_value_as_u32,
};
use num_bigint::BigInt;
use parking_lot::Mutex;
use serde_json::{Value, json};

#[derive(Debug, Parser)]
#[command(
    name = "neo-db-probe",
    about = "Read Neo contract storage from a RocksDB database without starting a node"
)]
struct Cli {
    #[arg(long, value_name = "PATH")]
    db: PathBuf,

    #[arg(long, value_name = "ADDRESS")]
    gas_address: Option<String>,

    #[arg(long, value_name = "HASH")]
    ledger_tx: Option<String>,

    #[arg(long, value_name = "HASH")]
    contract_state: Option<String>,

    #[arg(long, value_name = "HASH")]
    replay_tx: Option<String>,

    #[arg(long)]
    dump_contract_storage: bool,

    #[arg(long)]
    mpt_state_height: bool,

    #[arg(long, value_name = "HEIGHT")]
    mpt_state_root: Option<u32>,

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
    Nep17Account,
    HashIndex,
    TransactionState,
    ContractState,
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
    if cli.mpt_state_height || cli.mpt_state_root.is_some() {
        ensure_mpt_probe_args(&cli)?;
        let output = probe_mpt_state(&cli.db, cli.mpt_state_height, cli.mpt_state_root)
            .with_context(|| format!("read StateService MPT store {}", cli.db.display()))?;
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if let Some(tx_hash) = cli.replay_tx.as_deref() {
        ensure!(
            cli.gas_address.is_none()
                && cli.ledger_tx.is_none()
                && cli.contract_state.is_none()
                && cli.contract_id.is_none()
                && cli.key_base64.is_none()
                && cli.key_hex.is_none(),
            "--replay-tx cannot be combined with storage probe arguments"
        );
        let output = replay_transaction(&cli.db, tx_hash)
            .with_context(|| format!("replay transaction {tx_hash} from {}", cli.db.display()))?;
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    if cli.dump_contract_storage {
        ensure!(
            cli.gas_address.is_none()
                && cli.ledger_tx.is_none()
                && cli.contract_state.is_none()
                && cli.replay_tx.is_none()
                && cli.key_base64.is_none()
                && cli.key_hex.is_none()
                && cli.write_value_base64.is_none(),
            "--dump-contract-storage can only be combined with --contract-id and --dump-limit"
        );
        let contract_id = cli
            .contract_id
            .ok_or_else(|| anyhow!("--contract-id is required with --dump-contract-storage"))?;
        let output = dump_contract_storage(&cli.db, contract_id, cli.dump_limit)
            .with_context(|| format!("dump contract storage from {}", cli.db.display()))?;
        println!("{}", serde_json::to_string_pretty(&output)?);
        return Ok(());
    }

    let request = build_probe_request(&cli)?;
    let raw_key = storage_key_bytes(request.contract_id, &request.key_suffix);
    let written_len = if let Some(value) = cli.write_value_base64.as_deref() {
        let value = base64_decode(value).context("decode --write-value-base64")?;
        write_storage_value(
            &cli.db,
            request.contract_id,
            request.key_suffix.clone(),
            value.clone(),
        )
        .with_context(|| format!("write RocksDB at {}", cli.db.display()))?;
        Some(value.len())
    } else {
        None
    };
    let value = read_storage_value(&cli.db, request.contract_id, request.key_suffix.clone())
        .with_context(|| format!("read RocksDB at {}", cli.db.display()))?;

    let mut output = json!({
        "db": cli.db,
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
    ensure!(
        cli.gas_address.is_none()
            && cli.ledger_tx.is_none()
            && cli.contract_state.is_none()
            && cli.replay_tx.is_none()
            && !cli.dump_contract_storage
            && cli.contract_id.is_none()
            && cli.key_base64.is_none()
            && cli.key_hex.is_none()
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

fn probe_mpt_state(db_path: &Path, include_height: bool, root_index: Option<u32>) -> Result<Value> {
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: db_path.to_path_buf(),
        read_only: true,
        ..StorageConfig::default()
    });
    let store = provider.get_store("")?;
    let snapshot = store.snapshot();
    let mut output = json!({
        "db": db_path,
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

    Ok(output)
}

fn dump_contract_storage(db_path: &Path, contract_id: i32, limit: usize) -> Result<Value> {
    ensure!(limit > 0, "--dump-limit must be greater than zero");
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: db_path.to_path_buf(),
        read_only: true,
        ..StorageConfig::default()
    });
    let store = provider.get_store("")?;
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
        "contract_id": contract_id,
        "storage_prefix_hex": hex::encode(storage_key_prefix_bytes(contract_id)),
        "entry_count": entries.len(),
        "truncated": truncated,
        "entries": entries,
    }))
}

fn read_storage_value(
    db_path: &Path,
    contract_id: i32,
    key_suffix: Vec<u8>,
) -> Result<Option<Vec<u8>>> {
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: db_path.to_path_buf(),
        read_only: true,
        ..StorageConfig::default()
    });
    let store = provider.get_store("")?;
    let key = StorageKey::new(contract_id, key_suffix);
    Ok(store.try_get(&key).map(|item| item.to_value()))
}

fn write_storage_value(
    db_path: &Path,
    contract_id: i32,
    key_suffix: Vec<u8>,
    value: Vec<u8>,
) -> Result<()> {
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: db_path.to_path_buf(),
        read_only: false,
        ..StorageConfig::default()
    });
    let store = provider.get_store("")?;
    let mut snapshot = store.snapshot();
    let snapshot = Arc::get_mut(&mut snapshot)
        .ok_or_else(|| anyhow!("RocksDB snapshot is unexpectedly shared"))?;
    snapshot.put_sync(storage_key_bytes(contract_id, &key_suffix), value)?;
    snapshot.try_commit()?;
    Ok(())
}

fn open_store_cache(db_path: &Path) -> Result<StoreCache> {
    let provider = RocksDBStoreProvider::new(StorageConfig {
        path: db_path.to_path_buf(),
        read_only: true,
        ..StorageConfig::default()
    });
    let store = provider.get_store("")?;
    Ok(StoreCache::new_from_store(store, false))
}

fn replay_transaction(db_path: &Path, tx_hash: &str) -> Result<Value> {
    let tx_hash =
        UInt256::from_str(tx_hash).map_err(|err| anyhow!("invalid transaction hash: {err}"))?;
    neo_native_contracts::install();
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

    let store_cache = open_store_cache(db_path)?;
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

    let block_cache = Arc::new(snapshot.clone_cache());
    let tx_cache = Arc::new(block_cache.clone_cache());
    let container: Arc<dyn Verifiable> = Arc::new(transaction.clone());
    let mut engine = ApplicationEngine::new_with_shared_block(
        TriggerType::Application,
        Some(container),
        Arc::clone(&tx_cache),
        Some(Arc::clone(&block)),
        ProtocolSettings::mainnet(),
        transaction.system_fee(),
        diagnostic,
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
        "stored_vm_state": vm_state_name(tx_state.state),
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
    let StackValue::Struct(_, items) = value else {
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

fn decode_mpt_current_local_root_index(bytes: &[u8]) -> Result<u32> {
    ensure!(
        bytes.len() == 4,
        "current local root index has {} bytes, expected 4",
        bytes.len()
    );
    let arr: [u8; 4] = bytes.try_into().expect("length checked");
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
    let index = u32::from_le_bytes(bytes[1..5].try_into().expect("slice length checked"));
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

    let StackValue::Struct(_, items) = value else {
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
mod tests {
    use super::*;

    #[test]
    fn gas_account_key_from_address_matches_mainnet_sender_key() {
        let key =
            gas_account_key_from_address("NUDcRfftT99w4m2puzTxQToHxZPjQ9NN9n").expect("gas key");

        assert_eq!(base64_encode(&key), "FFsXKfRyjg82/UCteNWE46qLP5K8");
    }

    #[test]
    fn storage_key_bytes_prefix_contract_id_little_endian() {
        let bytes = storage_key_bytes(-6, &[0x14, 0xAA, 0xBB]);

        assert_eq!(hex::encode(bytes), "faffffff14aabb");
    }

    #[test]
    fn storage_key_prefix_bytes_uses_contract_id_without_suffix() {
        let bytes = storage_key_prefix_bytes(33);

        assert_eq!(hex::encode(bytes), "21000000");
    }

    #[test]
    fn mpt_state_root_key_uses_state_service_big_endian_height() {
        let key = mpt_state_root_key(474_701);

        assert_eq!(hex::encode(key), "0100073e4d");
    }

    #[test]
    fn mpt_current_local_root_index_key_matches_state_service_keyspace() {
        assert_eq!(hex::encode(mpt_current_local_root_index_key()), "02");
    }

    #[test]
    fn ledger_transaction_key_reverses_display_hash_for_storage() {
        let key = ledger_transaction_key_from_hash(
            "0xc68d5cad0e02197dd66623373751b84b2cadf742e79aaf836b53c6999a8d264d",
        )
        .expect("transaction key");

        assert_eq!(
            hex::encode(key),
            "0b4d268d9a99c6536b83af9ae742f7ad2c4bb85137372366d67d19020ead5c8dc6"
        );
    }

    #[test]
    fn contract_state_key_reverses_display_hash_for_storage() {
        let key = contract_state_key_from_hash("0xf970f4ccecd765b63732b821775dc38c25d74f23")
            .expect("contract state key");

        assert_eq!(
            hex::encode(key),
            "08234fd7258cc35d7721b83237b665d7ecccf470f9"
        );
    }

    #[test]
    fn decode_nep17_account_state_balance() {
        let bytes = base64_decode("QQEhBEGk/QI=").expect("base64");

        assert_eq!(
            decode_nep17_account_balance(&bytes).unwrap().to_string(),
            "50177089"
        );
    }

    #[test]
    fn decode_raw_bigint_uses_storage_integer_format() {
        let bytes = base64_decode("n0YM").expect("base64");

        assert_eq!(decode_raw_bigint(&bytes).to_string(), "804511");
        assert_eq!(decode_raw_bigint(&[]).to_string(), "0");
    }

    #[test]
    fn decode_hash_index_state_reads_ledger_current_block_pointer() {
        let bytes = base64_decode("QQIoIOVOraOmSo8jxSMutX/NUblHILNLvZdTGxS9ZpTsCjt6IQMaKAo=")
            .expect("base64");
        let state = decode_hash_index_state(&bytes).expect("hash index state");

        assert_eq!(state.index, 665626);
        assert_eq!(
            state.hash_hex_le,
            "e54eada3a64a8f23c5232eb57fcd51b94720b34bbd97531b14bd6694ec0a3b7a"
        );
    }

    #[test]
    fn decode_mpt_current_local_root_index_reads_little_endian_height() {
        let bytes = 474_701u32.to_le_bytes();

        assert_eq!(
            decode_mpt_current_local_root_index(&bytes).unwrap(),
            474_701
        );
    }

    #[test]
    fn decode_mpt_state_root_record_reads_unsigned_prefix_and_ignores_witness_tail() {
        let mut bytes = vec![0x00];
        bytes.extend_from_slice(&474_701u32.to_le_bytes());
        bytes.extend_from_slice(&[0xabu8; 32]);
        bytes.push(0x00);

        let decoded = decode_mpt_state_root_record(&bytes).expect("state-root record");

        assert_eq!(decoded["version"].as_u64(), Some(0));
        assert_eq!(decoded["index"].as_u64(), Some(474_701));
        let expected_hash = "ab".repeat(32);
        assert_eq!(
            decoded["roothash_hex_le"].as_str(),
            Some(expected_hash.as_str())
        );
        assert_eq!(decoded["trailing_bytes"].as_u64(), Some(1));
    }

    #[test]
    fn decode_transaction_state_reads_block_and_vm_state() {
        let mut tx = neo_payloads::Transaction::new();
        tx.set_system_fee(42);
        tx.set_network_fee(7);
        tx.set_valid_until_block(99);
        tx.set_signers(vec![neo_payloads::Signer::new(
            UInt160::zero(),
            neo_primitives::WitnessScope::CALLED_BY_ENTRY,
        )]);
        tx.set_script(vec![neo_vm_rs::OpCode::PUSH1.byte()]);
        tx.set_witnesses(vec![neo_payloads::Witness::new()]);
        let bytes = neo_native_contracts::LedgerContract::new()
            .serialize_persisted_transaction_state(12, VMState::FAULT, &tx)
            .expect("transaction state bytes");

        let state = decode_transaction_state(&bytes).expect("transaction state");

        assert_eq!(state.block_index, 12);
        assert_eq!(state.state, VMState::FAULT);
        let decoded_tx = state.transaction.expect("transaction");
        assert_eq!(decoded_tx.system_fee(), 42);
        assert_eq!(decoded_tx.network_fee(), 7);
        assert_eq!(decoded_tx.valid_until_block(), 99);
    }

    #[test]
    fn cli_accepts_negative_native_contract_id() {
        let cli = Cli::try_parse_from([
            "neo-db-probe",
            "--db",
            "data/mainnet",
            "--contract-id",
            "-4",
            "--key-hex",
            "0c",
        ])
        .expect("parse cli");

        assert_eq!(cli.contract_id, Some(-4));
    }

    #[test]
    fn cli_accepts_contract_storage_dump_without_key() {
        let cli = Cli::try_parse_from([
            "neo-db-probe",
            "--db",
            "data/mainnet",
            "--dump-contract-storage",
            "--contract-id",
            "33",
        ])
        .expect("parse cli");

        assert!(cli.dump_contract_storage);
        assert_eq!(cli.contract_id, Some(33));
    }

    #[test]
    fn cli_accepts_mpt_state_probe_without_contract_key() {
        let cli = Cli::try_parse_from([
            "neo-db-probe",
            "--db",
            "Data_MPT_validate_334F454E",
            "--mpt-state-height",
            "--mpt-state-root",
            "474701",
        ])
        .expect("parse cli");

        assert!(cli.mpt_state_height);
        assert_eq!(cli.mpt_state_root, Some(474_701));
    }

    #[test]
    fn mpt_state_probe_rejects_chain_storage_arguments() {
        let cli = Cli::try_parse_from([
            "neo-db-probe",
            "--db",
            "Data_MPT_validate_334F454E",
            "--mpt-state-height",
            "--contract-id",
            "-4",
            "--key-hex",
            "0c",
        ])
        .expect("parse cli");

        let err = ensure_mpt_probe_args(&cli).expect_err("mixed probe modes should fail");
        assert!(
            err.to_string()
                .contains("cannot be combined with chain storage probe arguments"),
            "{err}"
        );
    }
}
