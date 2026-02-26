use neo_core::neo_io::{MemoryReader, Serializable};
use neo_core::persistence::{
    providers::RocksDBStoreProvider, IStoreProvider, SeekDirection, StorageConfig, StorageKey,
    StoreCache,
};
use neo_core::smart_contract::binary_serializer::BinarySerializer;
use neo_core::smart_contract::contract_state::ContractState;
use neo_core::smart_contract::i_interoperable::IInteroperable;
use neo_core::smart_contract::native::ContractManagement;
use neo_core::UInt160;
use num_bigint::BigInt;
use std::collections::BTreeMap;
use std::path::PathBuf;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut args = std::env::args().skip(1);
    let path = args
        .next()
        .ok_or("usage: inspect_contract_state <db_path> <contract_hash> [contract_hash...]")?;
    let hashes: Vec<String> = args.collect();
    if hashes.is_empty() {
        return Err("at least one contract_hash is required".into());
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

    let next_id_key = StorageKey::new(-1, vec![15]);
    let count_key = StorageKey::new(-1, vec![16]);
    let min_fee_key = StorageKey::new(-1, vec![20]);
    for (label, key) in [
        ("next_id", next_id_key),
        ("contract_count", count_key),
        ("min_deploy_fee", min_fee_key),
    ] {
        if let Some(item) = cache.get(&key) {
            let bytes = item.get_value();
            let value = BigInt::from_signed_bytes_le(&bytes);
            println!(
                "meta {label}: bytes=0x{} value={}",
                hex::encode(bytes),
                value
            );
        } else {
            println!("meta {label}: <missing>");
        }
    }

    let prefix = StorageKey::new(-1, vec![8]);
    let mut id_hist = BTreeMap::<i32, usize>::new();
    let mut contracts_dump = Vec::<(i32, UInt160, String)>::new();
    let mut total_contract_entries = 0usize;
    let mut malformed = 0usize;
    for (key, item) in cache
        .data_cache()
        .find(Some(&prefix), SeekDirection::Forward)
    {
        let bytes = item.get_value();
        match ContractManagement::deserialize_contract_state(&bytes) {
            Ok(contract) => {
                if contract.id >= 0 {
                    total_contract_entries += 1;
                    *id_hist.entry(contract.id).or_insert(0) += 1;
                    contracts_dump.push((contract.id, contract.hash, contract.manifest.name));
                }
            }
            Err(err) => {
                malformed += 1;
                println!(
                    "malformed_contract_entry key=0x{} err={} len={} prefix=0x{}",
                    hex::encode(key.to_array()),
                    err,
                    bytes.len(),
                    hex::encode(&bytes[..bytes.len().min(24)])
                );
            }
        }
    }
    println!("list_contracts total={total_contract_entries} malformed={malformed}");
    for (id, count) in id_hist.iter().take(16) {
        println!("list_contracts id={id} count={count}");
    }
    if std::env::var_os("NEO_INSPECT_PRINT_ALL").is_some() {
        contracts_dump.sort_by_key(|(id, _, _)| *id);
        for (id, hash, name) in contracts_dump {
            println!("list_contract id={id} hash={hash} name={name}");
        }
    }

    for raw in hashes {
        let hash = UInt160::parse(raw.trim_start_matches("0x"))?;
        let key = StorageKey::new(-1, ContractManagement::contract_storage_key(&hash).to_vec());
        let Some(item) = cache.get(&key) else {
            println!("hash={hash} status=missing");
            continue;
        };
        let bytes = item.get_value();
        println!(
            "hash={hash} raw_len={} raw_prefix=0x{}",
            bytes.len(),
            hex::encode(&bytes[..bytes.len().min(24)])
        );

        match ContractManagement::deserialize_contract_state(&bytes) {
            Ok(state) => println!(
                "deserialize_contract_state: id={} updatecounter={} nef_checksum={} manifest_name={}",
                state.id, state.update_counter, state.nef.checksum, state.manifest.name
            ),
            Err(err) => println!("deserialize_contract_state: err={err}"),
        }

        match BinarySerializer::deserialize(&bytes, &neo_vm::ExecutionEngineLimits::default(), None)
        {
            Ok(stack) => {
                let mut from_stack = ContractState::default();
                match from_stack.from_stack_item(stack) {
                    Ok(()) => println!(
                        "from_stack_item: id={} updatecounter={} nef_checksum={} manifest_name={}",
                        from_stack.id,
                        from_stack.update_counter,
                        from_stack.nef.checksum,
                        from_stack.manifest.name
                    ),
                    Err(err) => println!("from_stack_item: err={err}"),
                }
            }
            Err(err) => println!("binary_deserialize: err={err}"),
        }

        let mut reader = MemoryReader::new(&bytes);
        match <ContractState as Serializable>::deserialize(&mut reader) {
            Ok(state) => println!(
                "serializable_deserialize: id={} updatecounter={} nef_checksum={} manifest_name={}",
                state.id, state.update_counter, state.nef.checksum, state.manifest.name
            ),
            Err(err) => println!("serializable_deserialize: err={err}"),
        }
    }

    Ok(())
}
