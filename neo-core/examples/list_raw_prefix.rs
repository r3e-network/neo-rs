use neo_core::persistence::{
    providers::RocksDBStoreProvider, IReadOnlyStoreGeneric, IStoreProvider, SeekDirection,
    StorageConfig,
};
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
    let path = args.next().ok_or("missing store path")?;
    let prefix_hex = args.next().ok_or("missing raw prefix hex argument")?;
    let limit = args
        .next()
        .unwrap_or_else(|| "50".to_string())
        .parse::<usize>()?;

    let prefix = decode_hex(&prefix_hex)?;
    let config = StorageConfig {
        path: PathBuf::from(path),
        read_only: true,
        ..Default::default()
    };

    let provider = RocksDBStoreProvider::new(config);
    let store = provider.get_store("")?;
    let snapshot = store.get_snapshot();

    let mut count = 0usize;
    let iter = <dyn neo_core::persistence::IStoreSnapshot as IReadOnlyStoreGeneric<
        Vec<u8>,
        Vec<u8>,
    >>::find(snapshot.as_ref(), Some(&prefix), SeekDirection::Forward);
    for (key, value) in iter.take(limit) {
        if !key.starts_with(&prefix) {
            break;
        }
        println!(
            "key=0x{} value_len={} value_hex=0x{}",
            hex::encode(&key),
            value.len(),
            hex::encode(&value)
        );
        count += 1;
    }
    println!("listed={count}");

    Ok(())
}
