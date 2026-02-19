use neo_core::persistence::{
    providers::RocksDBStoreProvider, IReadOnlyStoreGeneric, IStoreProvider, SeekDirection,
    StorageConfig, StoreCache,
};
use neo_core::smart_contract::StorageKey;
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
    let prefix_hex = args.next().ok_or("missing key prefix hex argument")?;
    let limit = args
        .next()
        .unwrap_or_else(|| "50".to_string())
        .parse::<usize>()?;
    let key_match_hex = args.next().map(|raw| {
        raw.strip_prefix("0x")
            .or_else(|| raw.strip_prefix("0X"))
            .unwrap_or(&raw)
            .to_ascii_lowercase()
    });

    let prefix = decode_hex(&prefix_hex)?;
    let config = StorageConfig {
        path: PathBuf::from(path),
        read_only: true,
        ..Default::default()
    };

    let provider = RocksDBStoreProvider::new(config);
    let store = provider.get_store("")?;
    let snapshot = store.get_snapshot();
    let cache = StoreCache::new_from_snapshot(snapshot);

    let prefix_key = StorageKey::new(id, prefix);
    let mut count = 0usize;
    for (key, item) in cache
        .find(Some(&prefix_key), SeekDirection::Forward)
        .take(limit)
    {
        let key_hex = hex::encode(key.to_array());
        if let Some(pattern) = key_match_hex.as_ref() {
            if !key_hex.contains(pattern) {
                continue;
            }
        }
        println!(
            "key=0x{} value_len={} value_hex=0x{}",
            key_hex,
            item.get_value().len(),
            hex::encode(item.get_value())
        );
        count += 1;
    }
    println!("listed={count}");

    Ok(())
}
