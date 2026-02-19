use neo_core::persistence::{
    providers::RocksDBStoreProvider, IStoreProvider, StorageConfig, StoreCache,
};
use neo_core::smart_contract::binary_serializer::BinarySerializer;
use neo_core::smart_contract::i_interoperable::IInteroperable;
use neo_core::smart_contract::native::account_state::AccountState;
use neo_core::smart_contract::StorageKey;
use neo_vm::execution_engine_limits::ExecutionEngineLimits;
use neo_vm::StackItem;
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
    let snapshot = store.get_snapshot();
    let cache = StoreCache::new_from_snapshot(snapshot);

    let key = StorageKey::new(id, key_suffix);
    let key_array = key.to_array();
    println!("key_hex=0x{}", hex::encode(&key_array));
    match cache.get(&key) {
        Some(item) => {
            let raw = item.get_value();
            println!("value_hex=0x{}", hex::encode(&raw));
            println!("value_bigint={}", item.to_bigint());
            match BinarySerializer::deserialize(&raw, &ExecutionEngineLimits::default(), None) {
                Ok(StackItem::Struct(_)) => {
                    let mut account_state = AccountState::default();
                    if account_state.from_stack_item(
                        BinarySerializer::deserialize(&raw, &ExecutionEngineLimits::default(), None)
                            .unwrap_or_else(|_| StackItem::null()),
                    )
                    .is_ok()
                    {
                        println!("value_account_balance={}", account_state.balance);
                    }
                }
                _ => {}
            }
        }
        None => println!("value_hex=<none>"),
    }
    Ok(())
}
