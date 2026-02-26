use neo_core::persistence::{
    providers::RocksDBStoreProvider, IStoreProvider, StorageConfig, StoreCache,
};
use neo_core::smart_contract::native::ledger_contract::{HashOrIndex, LedgerContract};
use std::path::PathBuf;

fn parse_u32_arg(value: Option<String>, default: u32) -> Result<u32, String> {
    match value {
        Some(raw) => raw
            .parse::<u32>()
            .map_err(|err| format!("invalid u32 value `{raw}`: {err}")),
        None => Ok(default),
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: find_txs_by_script_text <db_path> <needle_text> [start] [end]")?;
    let needle = args
        .next()
        .ok_or("usage: find_txs_by_script_text <db_path> <needle_text> [start] [end]")?;
    if needle.is_empty() {
        return Err("needle_text cannot be empty".into());
    }

    let config = StorageConfig {
        path: PathBuf::from(path),
        read_only: true,
        ..Default::default()
    };
    let provider = RocksDBStoreProvider::new(config);
    let store = provider.get_store("")?;
    let snapshot = store.get_snapshot();
    let cache = StoreCache::new_from_snapshot(snapshot);
    let ledger = LedgerContract::new();

    let current = ledger.current_index(&cache)?;
    let start = parse_u32_arg(args.next(), 0)?;
    let end = parse_u32_arg(args.next(), current)?;
    if start > end {
        return Err(format!("invalid range: start ({start}) > end ({end})").into());
    }

    let needle_bytes = needle.as_bytes();
    println!("needle={needle} start={start} end={end} tip={current}");

    let mut scanned = 0u64;
    let mut hits = 0usize;
    for index in start..=end {
        scanned += 1;
        if scanned % 50_000 == 0 {
            eprintln!("scan_progress blocks={scanned} current_index={index} hits={hits}");
        }

        let Some(block) = ledger.get_block(&cache, HashOrIndex::Index(index))? else {
            continue;
        };

        for tx in block.transactions {
            let script = tx.script();
            if !script
                .windows(needle_bytes.len())
                .any(|window| window == needle_bytes)
            {
                continue;
            }
            let tx_hash = tx.hash();
            let tx_state = ledger.get_transaction_state(&cache, &tx_hash)?;
            let (vm_state, vm_state_raw, tx_block_index) = if let Some(state) = tx_state {
                (
                    format!("{:?}", state.vm_state()),
                    state.vm_state_raw(),
                    state.block_index(),
                )
            } else {
                ("<missing>".to_string(), 0, 0)
            };
            println!(
                "hit block={} tx_block={} tx={} vm_state={} vm_state_raw={} sys_fee={} net_fee={} sender={}",
                index,
                tx_block_index,
                tx_hash,
                vm_state,
                vm_state_raw,
                tx.system_fee(),
                tx.network_fee(),
                tx.sender().map(|sender| sender.to_string()).unwrap_or_else(|| "<none>".to_string())
            );
            hits += 1;
        }
    }

    println!("done scanned_blocks={scanned} hits={hits}");
    Ok(())
}
