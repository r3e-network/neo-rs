use super::*;
use crate::hashes::CRYPTO_LIB_HASH;

#[test]
fn provider_registers_exact_standard_catalog() {
    let provider = StandardNativeProvider::new();
    let specs = crate::standard_native_contract_specs();
    let contracts = provider.all_native_contracts();

    assert_eq!(contracts.len(), crate::STANDARD_NATIVE_CONTRACT_COUNT);
    assert_eq!(contracts.len(), specs.len());

    for (contract, spec) in contracts.iter().zip(specs) {
        assert_eq!(contract.id(), spec.id, "{} id", spec.name);
        assert_eq!(contract.name(), spec.name, "{} name", spec.name);
        assert_eq!(contract.hash(), spec.hash, "{} hash", spec.name);
        assert_eq!(
            provider
                .get_native_contract(&spec.hash)
                .expect("hash lookup")
                .name(),
            spec.name,
            "{} hash lookup",
            spec.name
        );
    }

    let expected_hashes = specs.iter().map(|spec| spec.hash).collect::<Vec<_>>();
    assert_eq!(provider.all_native_contract_hashes(), expected_hashes);
}

#[test]
fn provider_resolves_cryptolib_by_hash() {
    let provider = StandardNativeProvider::new();

    let by_hash = provider
        .get_native_contract(&CRYPTO_LIB_HASH)
        .expect("CryptoLib resolvable by hash");
    assert_eq!(by_hash.name(), "CryptoLib");
    assert_eq!(by_hash.id(), -3);
}

#[test]
fn provider_current_block_index_feeds_engine_without_persisting_block() {
    use crate::LedgerContract;
    use neo_config::ProtocolSettings;
    use neo_execution::ApplicationEngine;
    use neo_payloads::Block;
    use neo_primitives::{TriggerType, UInt256};
    use neo_storage::StorageItem;
    use neo_storage::persistence::DataCache;
    use std::sync::Arc;

    let cache = Arc::new(DataCache::new(false));
    let current_hash = UInt256::from_bytes(&[0x34; 32]).unwrap();
    cache.add(
        LedgerContract::current_block_storage_key(),
        StorageItem::from_bytes(
            LedgerContract::new()
                .serialize_hash_index_state(&current_hash, 1234)
                .unwrap(),
        ),
    );

    let engine = ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        None,
        Arc::clone(&cache),
        None::<Block>,
        ProtocolSettings::default(),
        1_000_000,
        neo_execution::NoDiagnostic,
        Some(std::sync::Arc::new(crate::StandardNativeProvider::new())),
    )
    .expect("engine builds");

    assert_eq!(engine.current_block_index(), 1234);
}
