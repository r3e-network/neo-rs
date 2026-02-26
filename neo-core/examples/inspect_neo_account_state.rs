use neo_core::persistence::{
    providers::RocksDBStoreProvider, IStoreProvider, SeekDirection, StorageConfig, StorageKey,
    StoreCache,
};
use neo_core::smart_contract::binary_serializer::BinarySerializer;
use neo_core::UInt160;
use neo_vm::execution_engine_limits::ExecutionEngineLimits;
use neo_vm::StackItem;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: inspect_neo_account_state <db_path> <account_hash>")?;
    let account_raw = args
        .next()
        .ok_or("usage: inspect_neo_account_state <db_path> <account_hash>")?;
    let account = UInt160::parse(account_raw.trim_start_matches("0x"))?;

    let config = StorageConfig {
        path: PathBuf::from(path),
        read_only: true,
        ..Default::default()
    };
    let provider = RocksDBStoreProvider::new(config);
    let store = provider.get_store("")?;
    let snapshot = store.get_snapshot();
    let cache = StoreCache::new_from_snapshot(snapshot);

    let key = StorageKey::create_with_uint160(-5, 20, &account);
    println!("key_hex=0x{}", hex::encode(key.to_array()));
    let Some(item) = cache.get(&key) else {
        println!("account_state=<missing>");
        return Ok(());
    };
    let raw = item.get_value();
    println!("raw_hex=0x{}", hex::encode(&raw));
    println!("raw_bigint={}", item.to_bigint());

    let stack = BinarySerializer::deserialize(&raw, &ExecutionEngineLimits::default(), None)?;
    match stack {
        StackItem::Struct(s) => {
            let entries = s.items();
            println!("entries_len={}", entries.len());
            if entries.len() >= 4 {
                println!(
                    "balance={} balance_height={} vote_to={} last_gas_per_vote={}",
                    entries[0].as_int()?,
                    entries[1].as_int()?,
                    if entries[2].is_null() {
                        "<null>".to_string()
                    } else if let Ok(bytes) = entries[2].as_bytes() {
                        format!("0x{}", hex::encode(bytes))
                    } else {
                        format!("{:?}", entries[2].stack_item_type())
                    },
                    entries[3].as_int()?
                );
            }
        }
        other => println!("unexpected_stack_type={:?}", other.stack_item_type()),
    }

    let prefix = StorageKey::create(-5, 29);
    println!("gas_per_block_records:");
    for (index, (k, v)) in cache
        .data_cache()
        .find(Some(&prefix), SeekDirection::Backward)
        .take(10)
        .enumerate()
    {
        let suffix = k.suffix();
        let idx = if suffix.len() >= 5 {
            let b = &suffix[suffix.len() - 4..];
            u32::from_be_bytes([b[0], b[1], b[2], b[3]])
        } else {
            0
        };
        println!(
            "  [{index}] key=0x{} suffix=0x{} parsed_index={} value={}",
            hex::encode(k.to_array()),
            hex::encode(suffix),
            idx,
            v.to_bigint()
        );
    }

    Ok(())
}
