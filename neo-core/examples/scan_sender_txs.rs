use neo_core::persistence::{
    providers::RocksDBStoreProvider, IStoreProvider, StorageConfig, StoreCache,
};
use neo_core::smart_contract::native::ledger_contract::{HashOrIndex, LedgerContract};
use neo_core::UInt160;
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
    let path = args.next().unwrap_or_else(|| "./data/testnet".to_string());
    let sender_raw = args
        .next()
        .ok_or("usage: scan_sender_txs <db_path> <sender_hash> [start] [end]")?;
    let sender = UInt160::parse(sender_raw.trim())
        .map_err(|err| format!("invalid sender hash `{sender_raw}`: {err}"))?;

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

    println!("sender={sender} start={start} end={end} tip={current}");

    let mut hits = 0usize;
    let mut scanned = 0u64;
    for index in start..=end {
        scanned += 1;
        if scanned % 50_000 == 0 {
            eprintln!("scan_progress blocks={scanned} current_index={index} hits={hits}");
        }

        let Some(block) = ledger.get_block(&cache, HashOrIndex::Index(index))? else {
            continue;
        };

        for tx in block.transactions {
            if tx.sender() != Some(sender) {
                continue;
            }
            let tx_hash = tx.hash();
            let state = ledger.get_transaction_state(&cache, &tx_hash)?;
            let (vm_state, vm_state_raw, tx_block_index) = if let Some(state) = state {
                (
                    format!("{:?}", state.vm_state()),
                    state.vm_state_raw(),
                    state.block_index(),
                )
            } else {
                ("<missing>".to_string(), 0u8, 0u32)
            };
            println!(
                "hit block={} tx_block={} tx={} sys_fee={} net_fee={} vm_state={} vm_state_raw={} signers={}",
                index,
                tx_block_index,
                tx_hash,
                tx.system_fee(),
                tx.network_fee(),
                vm_state,
                vm_state_raw,
                tx.signers().len()
            );
            hits += 1;
        }
    }

    println!("done scanned_blocks={scanned} hits={hits}");
    Ok(())
}
