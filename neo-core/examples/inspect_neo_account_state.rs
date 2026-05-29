use neo_core::persistence::{
    providers::RocksDBStoreProvider, SeekDirection, StorageConfig, StorageItemExt, StorageKey,
    StoreCache, StoreProvider,
};
use neo_core::smart_contract::BinarySerializer;
use neo_core::UInt160;
use neo_vm_rs::StackValue;
use num_bigint::BigInt;
use std::path::PathBuf;

fn stack_value_bigint(value: &StackValue) -> Option<BigInt> {
    match value {
        StackValue::Integer(value) => Some(BigInt::from(*value)),
        StackValue::BigInteger(bytes)
        | StackValue::ByteString(bytes)
        | StackValue::Buffer(bytes) => Some(BigInt::from_signed_bytes_le(bytes)),
        StackValue::Boolean(value) => Some(BigInt::from(u8::from(*value))),
        StackValue::Null => Some(BigInt::from(0)),
        _ => None,
    }
}

fn stack_value_bytes(value: &StackValue) -> Option<&[u8]> {
    match value {
        StackValue::ByteString(bytes) | StackValue::Buffer(bytes) => Some(bytes),
        _ => None,
    }
}

fn stack_value_type(value: &StackValue) -> &'static str {
    match value {
        StackValue::Integer(_) => "Integer",
        StackValue::BigInteger(_) => "BigInteger",
        StackValue::ByteString(_) => "ByteString",
        StackValue::Buffer(_) => "Buffer",
        StackValue::Boolean(_) => "Boolean",
        StackValue::Array(_) => "Array",
        StackValue::Struct(_) => "Struct",
        StackValue::Map(_) => "Map",
        StackValue::Interop(_) => "Interop",
        StackValue::Iterator(_) => "Iterator",
        StackValue::Null => "Null",
        StackValue::Pointer(_) => "Pointer",
    }
}

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
    let snapshot = store.snapshot();
    let cache = StoreCache::new_from_snapshot(snapshot);

    let key = StorageKey::create_with_uint160(-5, 20, &account);
    println!("key_hex=0x{}", hex::encode(key.to_array()));
    let Some(item) = cache.get(&key) else {
        println!("account_state=<missing>");
        return Ok(());
    };
    let raw = item.to_value();
    println!("raw_hex=0x{}", hex::encode(&raw));
    println!("raw_bigint={}", item.to_bigint());

    let stack = BinarySerializer::deserialize_stack_value(&raw)?;
    match stack {
        StackValue::Struct(entries) => {
            println!("entries_len={}", entries.len());
            if entries.len() >= 4 {
                println!(
                    "balance={} balance_height={} vote_to={} last_gas_per_vote={}",
                    stack_value_bigint(&entries[0]).ok_or("invalid balance")?,
                    stack_value_bigint(&entries[1]).ok_or("invalid balance height")?,
                    if matches!(entries[2], StackValue::Null) {
                        "<null>".to_string()
                    } else if let Some(bytes) = stack_value_bytes(&entries[2]) {
                        format!("0x{}", hex::encode(bytes))
                    } else {
                        stack_value_type(&entries[2]).to_string()
                    },
                    stack_value_bigint(&entries[3]).ok_or("invalid last gas per vote")?
                );
            }
        }
        other => println!("unexpected_stack_value_type={}", stack_value_type(&other)),
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
