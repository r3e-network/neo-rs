use neo_core::persistence::{
    providers::RocksDBStoreProvider, StoreProvider, StorageConfig, StoreCache,
};
use neo_core::smart_contract::BinarySerializer;
use neo_core::smart_contract::StorageKey;
use neo_vm_rs::StackValue;
use num_bigint::BigInt;
use std::path::PathBuf;

fn decode_hex(input: &str) -> Result<Vec<u8>, String> {
    let trimmed = input
        .strip_prefix("0x")
        .or_else(|| input.strip_prefix("0X"))
        .unwrap_or(input);
    if trimmed.is_empty() {
        return Ok(Vec::new());
    }
    hex::decode(trimmed).map_err(|err| format!("invalid hex `{input}`: {err}"))
}

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

fn account_balance_from_stack_value(value: &StackValue) -> Option<BigInt> {
    match value {
        StackValue::Struct(entries) => entries.first().and_then(stack_value_bigint),
        _ => None,
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args.next().unwrap_or_else(|| "./data/testnet".to_string());
    let id = args
        .next()
        .ok_or("missing contract id argument")?
        .parse::<i32>()?;
    let key_hex = args.next().ok_or("missing storage key hex argument")?;
    let key_suffix = decode_hex(&key_hex)?;

    let config = StorageConfig {
        path: PathBuf::from(path),
        read_only: true,
        ..Default::default()
    };

    let provider = RocksDBStoreProvider::new(config);
    let store = provider.get_store("")?;
    let snapshot = store.snapshot();
    let cache = StoreCache::new_from_snapshot(snapshot);

    let key = StorageKey::new(id, key_suffix);
    let key_array = key.to_array();
    println!("key_hex=0x{}", hex::encode(&key_array));
    match cache.get(&key) {
        Some(item) => {
            let raw = item.to_value();
            println!("value_hex=0x{}", hex::encode(&raw));
            println!("value_bigint={}", item.to_bigint());
            if let Ok(stack_value) = BinarySerializer::deserialize_stack_value(&raw) {
                if let Some(balance) = account_balance_from_stack_value(&stack_value) {
                    println!("value_account_balance={balance}");
                }
            }
        }
        None => println!("value_hex=<none>"),
    }
    Ok(())
}
