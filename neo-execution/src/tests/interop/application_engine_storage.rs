use super::*;
use crate::NoDiagnostic;
use crate::native_contract_provider::NoNativeContractProvider;
use neo_config::ProtocolSettings;
use neo_primitives::TriggerType;
use neo_storage::persistence::DataCache;
use neo_vm_rs::OpCode;
use std::sync::Arc;

fn storage_engine() -> ApplicationEngine {
    let mut engine =
        ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            1_000_000,
            NoDiagnostic,
            Arc::new(NoNativeContractProvider),
        )
        .expect("engine builds");
    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("script loads");
    engine
}

#[test]
fn storage_get_delete_find_allow_oversized_keys_like_csharp() {
    let mut engine = storage_engine();
    let context = StorageContext::read_write(42);
    let oversized_key = vec![0xAA; neo_primitives::constants::MAX_STORAGE_KEY_SIZE + 1];

    assert_eq!(
        engine
            .storage_get(context.clone(), oversized_key.clone())
            .expect("oversized Storage.Get should not fault"),
        None
    );
    engine
        .storage_delete(context.clone(), oversized_key.clone())
        .expect("oversized Storage.Delete should not fault");
    engine
        .storage_find(context, oversized_key, FindOptions::None)
        .expect("oversized Storage.Find prefix should not fault");
}

#[test]
fn storage_put_still_enforces_max_key_size() {
    let mut engine = storage_engine();
    let context = StorageContext::read_write(42);
    let oversized_key = vec![0xAA; neo_primitives::constants::MAX_STORAGE_KEY_SIZE + 1];

    assert!(
        engine
            .storage_put(context, oversized_key, vec![0x01])
            .is_err(),
        "Storage.Put is the C# path that enforces MaxStorageKeySize"
    );
}
