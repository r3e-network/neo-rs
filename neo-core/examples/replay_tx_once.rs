use neo_core::persistence::data_cache::DataCacheConfig;
use neo_core::persistence::{
    providers::RocksDBStoreProvider, DataCache, IStoreProvider, SeekDirection, StorageConfig,
    StoreCache,
};
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::{ApplicationEngine, TEST_MODE_GAS};
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::native::ledger_contract::{HashOrIndex, LedgerContract};
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::smart_contract::{StorageItem, StorageKey};
use neo_core::{UInt256, IVerifiable};
use std::path::PathBuf;
use std::sync::Arc;

fn print_instruction_window(context: &neo_vm::execution_context::ExecutionContext) {
    let script = context.script();
    let target_ip = context.instruction_pointer();
    let mut decoded = Vec::new();
    let mut position = 0usize;

    while position < script.len() {
        let instruction = match script.get_instruction(position) {
            Ok(instruction) => instruction,
            Err(_) => break,
        };
        let opcode = instruction.opcode();
        let size = instruction.size();
        decoded.push((position, opcode, size));
        if size == 0 {
            break;
        }
        position += size;
    }

    let center = decoded
        .iter()
        .position(|(offset, _, _)| *offset == target_ip)
        .unwrap_or_else(|| decoded.len().saturating_sub(1));
    let nearest_initslot = decoded
        .iter()
        .rev()
        .find(|(offset, opcode, _)| {
            *offset <= target_ip
                && (*opcode == neo_vm::op_code::OpCode::INITSLOT
                    || *opcode == neo_vm::op_code::OpCode::INITSSLOT)
        })
        .copied();
    if let Some((offset, opcode, _)) = nearest_initslot {
        println!("nearest slot init before fault: offset={offset} opcode={opcode:?}");
    } else {
        println!("nearest slot init before fault: <none>");
    }
    let start = center.saturating_sub(3);
    let end = (center + 4).min(decoded.len());
    println!("context script window around ip={target_ip}:");
    for (offset, opcode, size) in decoded[start..end].iter().copied() {
        let marker = if offset == target_ip { ">>" } else { "  " };
        println!("{marker} offset={offset} opcode={opcode:?} size={size}");
    }
}

fn open_cache(path: &str) -> Result<StoreCache, Box<dyn std::error::Error>> {
    let config = StorageConfig {
        path: PathBuf::from(path),
        read_only: true,
        ..Default::default()
    };
    let provider = RocksDBStoreProvider::new(config);
    let store = provider.get_store("")?;
    Ok(StoreCache::new_from_snapshot(store.get_snapshot()))
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let target_state_dir = args
        .next()
        .ok_or("usage: replay_tx_once <target_state_dir> <tx_source_dir> <tx_hash> [context_block_index]")?;
    let tx_source_dir = args
        .next()
        .ok_or("usage: replay_tx_once <target_state_dir> <tx_source_dir> <tx_hash> [context_block_index]")?;
    let tx_hash_raw = args
        .next()
        .ok_or("usage: replay_tx_once <target_state_dir> <tx_source_dir> <tx_hash> [context_block_index]")?;
    let tx_hash = UInt256::parse(tx_hash_raw.trim_start_matches("0x"))?;

    let source_cache = open_cache(&tx_source_dir)?;
    let target_cache = open_cache(&target_state_dir)?;
    let ledger = LedgerContract::new();
    let skip_on_persist = std::env::var("NEO_REPLAY_SKIP_ON_PERSIST")
        .ok()
        .map(|raw| {
            let normalized = raw.trim().to_ascii_lowercase();
            matches!(normalized.as_str(), "1" | "true" | "yes" | "on")
        })
        .unwrap_or(false);

    let tx_state = ledger
        .get_transaction_state(&source_cache, &tx_hash)?
        .ok_or("tx not found in tx_source_dir")?;
    let context_block_index = if let Some(raw) = args.next() {
        raw.parse::<u32>()?
    } else {
        tx_state.block_index()
    };
    let block = ledger
        .get_block(&source_cache, HashOrIndex::Index(context_block_index))?
        .ok_or("context block not found in tx_source_dir")?;
    let tx = tx_state.transaction().clone();

    println!(
        "tx={} tx_block_index={} context_block_index={} sys_fee={} net_fee={}",
        tx_hash,
        tx_state.block_index(),
        context_block_index,
        tx.system_fee(),
        tx.network_fee()
    );

    let base_snapshot = Arc::new(target_cache.data_cache().clone());
    let protocol = ProtocolSettings::testnet();
    let block_ref = Arc::new(block);

    if !skip_on_persist {
        let mut on_persist_engine = ApplicationEngine::new_with_shared_block(
            TriggerType::OnPersist,
            None,
            Arc::clone(&base_snapshot),
            Some(Arc::clone(&block_ref)),
            protocol.clone(),
            TEST_MODE_GAS,
            None,
        )?;
        on_persist_engine.native_on_persist()?;
    } else {
        println!("on_persist=skipped");
    }

    let tx_store_get: Arc<dyn Fn(&StorageKey) -> Option<StorageItem> + Send + Sync> = {
        let base = Arc::clone(&base_snapshot);
        Arc::new(move |key: &StorageKey| base.get(key))
    };
    let tx_store_find: Arc<
        dyn Fn(Option<&StorageKey>, SeekDirection) -> Vec<(StorageKey, StorageItem)> + Send + Sync,
    > = {
        let base = Arc::clone(&base_snapshot);
        Arc::new(move |prefix: Option<&StorageKey>, direction: SeekDirection| {
            base.find(prefix, direction).collect::<Vec<_>>()
        })
    };
    let tx_snapshot = Arc::new(DataCache::new_with_config(
        false,
        Some(tx_store_get),
        Some(tx_store_find),
        DataCacheConfig {
            track_reads_in_write_cache: false,
            enable_read_cache: false,
            enable_prefetching: false,
            ..Default::default()
        },
    ));

    let container: Arc<dyn IVerifiable> = Arc::new(tx.clone());
    let mut tx_engine = ApplicationEngine::new_with_shared_block(
        TriggerType::Application,
        Some(container),
        tx_snapshot,
        Some(block_ref),
        protocol,
        tx.system_fee(),
        None,
    )?;
    tx_engine.load_script(tx.script().to_vec(), CallFlags::ALL, None)?;
    let vm_state = tx_engine.execute_allow_fault();

    println!("vm_state={vm_state:?}");
    println!(
        "fault_exception={}",
        tx_engine.fault_exception().unwrap_or("<none>")
    );
    println!(
        "invocation_depth={}",
        tx_engine.invocation_stack().len()
    );
    for (index, context) in tx_engine.invocation_stack().iter().enumerate() {
        let opcode = context
            .current_instruction()
            .map(|instruction| format!("{:?}", instruction.opcode()))
            .unwrap_or_else(|_| "<end>".to_string());
        println!(
            "context[{index}] script_hash=0x{} ip={} opcode={} eval_depth={} arg_count={} local_count={} static_count={}",
            hex::encode(context.script_hash()),
            context.instruction_pointer(),
            opcode,
            context.evaluation_stack().len(),
            context.arguments().map(|slot| slot.len()).unwrap_or(0),
            context.local_variables().map(|slot| slot.len()).unwrap_or(0),
            context.static_fields_len(),
        );
        if index + 1 == tx_engine.invocation_stack().len() {
            print_instruction_window(context);
        }
    }
    println!(
        "gas_consumed={} fee_consumed={} exec_fee_factor_raw={} storage_price={}",
        tx_engine.gas_consumed(),
        tx_engine.fee_consumed(),
        tx_engine.exec_fee_factor_raw(),
        tx_engine.storage_price()
    );

    Ok(())
}
