use super::*;
use neo_config::{Hardfork, ProtocolSettings};
use neo_execution::native_contract::build_native_contract_state;
use neo_payloads::signer::Signer;
use neo_payloads::transaction::Transaction;
use neo_payloads::witness::Witness;
use neo_payloads::{Block, BlockHeader, VerifiableContainer};
use neo_primitives::{CallFlags, TriggerType, WitnessScope};
use neo_vm::StackItem;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::VmState;
use std::sync::Arc;

/// Writes a serialized contract record under `Prefix_Contract ++ hash`.
fn put_contract_record(cache: &DataCache, state: &ContractState) {
    cache.add(
        ContractManagement::contract_storage_key(&state.hash),
        StorageItem::from_bytes(state.serialize_contract_record().expect("record bytes")),
    );
}

/// Builds the entry script `System.Contract.Call(CM, "destroy", [])`.
fn destroy_script() -> Vec<u8> {
    let mut builder = ScriptBuilder::new();
    builder.emit_push_int(0);
    builder.emit_pack();
    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push("destroy".as_bytes());
    builder.emit_push(&ContractManagement::script_hash().to_array());
    builder
        .emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call");
    builder.to_array()
}

fn engine_for(
    snapshot: Arc<DataCache>,
    persisting_block: Option<Block>,
    settings: ProtocolSettings,
) -> ApplicationEngine<crate::StandardNativeProvider> {
    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
    tx.set_witnesses(vec![Witness::empty()]);
    let container = Arc::new(VerifiableContainer::from(tx));
    ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        Some(container),
        snapshot,
        persisting_block,
        settings,
        100_00000000,
        neo_execution::NoDiagnostic,
        Some(std::sync::Arc::new(crate::StandardNativeProvider::new())),
    )
    .expect("engine builds")
}

#[test]
fn destroy_removes_record_index_storage_and_blocks_hash() {
    let cache = DataCache::new(false);
    // Seed the ContractManagement native record so System.Contract.Call
    // resolves the callee.
    put_contract_record(
        &cache,
        &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
    );

    // The entry script IS the calling contract: pin its hash, then deploy
    // a user contract under that hash (record + id index + one storage
    // row + one Policy whitelist entry).
    let script = destroy_script();
    let self_hash = UInt160::from_script(&script);
    let user = ContractState::new_native(7, self_hash, "SelfDestructFixture".to_string());
    put_contract_record(&cache, &user);
    let index_key = ContractManagement::contract_id_storage_key(7);
    cache.add(
        index_key.clone(),
        StorageItem::from_bytes(self_hash.to_bytes().to_vec()),
    );
    let user_row = StorageKey::new(7, vec![0x01]);
    cache.add(user_row.clone(), StorageItem::from_bytes(vec![0xEE]));
    // A whitelist entry for the contract (C# WhitelistedContract
    // Struct[ContractHash, Method, ArgCount, FixedFee]) that CleanWhitelist
    // must remove and report.
    let wl_key = crate::PolicyContract::whitelist_fee_key(&self_hash, 0);
    let wl_value = BinarySerializer::serialize(
        &StackItem::from_struct(vec![
            StackItem::from_byte_string(self_hash.to_bytes()),
            StackItem::from_byte_string("transfer".as_bytes().to_vec()),
            StackItem::from_int(4),
            StackItem::from_int(0),
        ]),
        &ExecutionEngineLimits::default(),
    )
    .unwrap();
    cache.add(wl_key.clone(), StorageItem::from_bytes(wl_value));
    let snapshot = Arc::new(cache);

    // Default MainNet schedules Faun at 8,800,000, so height 0 runs the
    // pre-Faun BlockAccountInternal branch (empty blocked value).
    // The destroy path reads the persisting block's timestamp, so the
    // engine needs a persisting block fixture (height 0, pre-Faun).
    let mut persisting_header = BlockHeader::default();
    persisting_header.set_index(0);
    persisting_header.set_timestamp(1_700_000_000_000);
    let persisting_block = Some(Block::from_parts(persisting_header, vec![]));
    let mut engine = engine_for(
        Arc::clone(&snapshot),
        persisting_block,
        ProtocolSettings::default(),
    );
    engine
        .load_script(script, CallFlags::ALL, Some(self_hash))
        .expect("script loads");
    assert_eq!(
        engine.execute_allow_fault(),
        VmState::HALT,
        "destroy must HALT"
    );

    // The contract record, id index, and contract storage are gone.
    assert!(
        snapshot
            .get(&ContractManagement::contract_storage_key(&self_hash))
            .is_none(),
        "contract record deleted"
    );
    assert!(
        snapshot.get(&index_key).is_none(),
        "id->hash index entry deleted"
    );
    assert!(
        snapshot.get(&user_row).is_none(),
        "contract storage deleted"
    );
    // The destroyed hash is locked via Policy's blocked-account entry,
    // pre-Faun with an EMPTY value (C# StorageItem([])).
    let blocked = snapshot
        .get(&crate::PolicyContract::blocked_account_key(&self_hash))
        .expect("destroyed contract is blocked");
    assert!(
        blocked.value_bytes().is_empty(),
        "pre-Faun blocked value is empty"
    );
    // The whitelist entry was cleaned.
    assert!(snapshot.get(&wl_key).is_none(), "whitelist entry deleted");

    // Events: Policy's WhitelistFeeChanged for the cleaned entry, then
    // ContractManagement's Destroy with the destroyed hash.
    let notifications = engine.notifications();
    let destroy_event = notifications
        .iter()
        .find(|n| n.event_name == "Destroy")
        .expect("Destroy event emitted");
    assert_eq!(destroy_event.script_hash, ContractManagement::script_hash());
    assert_eq!(
        destroy_event.state[0].as_bytes().unwrap(),
        self_hash.to_bytes().to_vec()
    );
    let wl_event = notifications
        .iter()
        .find(|n| n.event_name == "WhitelistFeeChanged")
        .expect("WhitelistFeeChanged event emitted");
    assert_eq!(wl_event.script_hash, crate::PolicyContract::script_hash());
    assert_eq!(wl_event.state[1].as_bytes().unwrap(), b"transfer".to_vec());
    assert_eq!(wl_event.state[2].as_int().unwrap(), BigInt::from(4));
    assert!(matches!(wl_event.state[3], StackItem::Null));
}

/// Protocol settings with every hardfork through `HF_Gorgon` active at height 0,
/// so `destroy` runs the post-Gorgon block-*before*-erase path.
fn settings_with_gorgon() -> ProtocolSettings {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.clear();
    for hf in [
        Hardfork::HfAspidochelone,
        Hardfork::HfBasilisk,
        Hardfork::HfCockatrice,
        Hardfork::HfDomovoi,
        Hardfork::HfEchidna,
        Hardfork::HfFaun,
        Hardfork::HfGorgon,
    ] {
        settings.hardforks.insert(hf, 0);
    }
    settings
}

#[test]
fn destroy_under_gorgon_blocks_before_erase_and_reaches_the_same_final_state() {
    let settings = settings_with_gorgon();
    let cache = DataCache::new(false);
    put_contract_record(
        &cache,
        &build_native_contract_state(&ContractManagement, &settings, 0),
    );

    let script = destroy_script();
    let self_hash = UInt160::from_script(&script);
    let user = ContractState::new_native(7, self_hash, "SelfDestructFixture".to_string());
    put_contract_record(&cache, &user);
    let index_key = ContractManagement::contract_id_storage_key(7);
    cache.add(
        index_key.clone(),
        StorageItem::from_bytes(self_hash.to_bytes().to_vec()),
    );
    let user_row = StorageKey::new(7, vec![0x01]);
    cache.add(user_row.clone(), StorageItem::from_bytes(vec![0xEE]));
    let wl_key = crate::PolicyContract::whitelist_fee_key(&self_hash, 0);
    let wl_value = BinarySerializer::serialize(
        &StackItem::from_struct(vec![
            StackItem::from_byte_string(self_hash.to_bytes()),
            StackItem::from_byte_string("transfer".as_bytes().to_vec()),
            StackItem::from_int(4),
            StackItem::from_int(0),
        ]),
        &ExecutionEngineLimits::default(),
    )
    .unwrap();
    cache.add(wl_key.clone(), StorageItem::from_bytes(wl_value));
    let snapshot = Arc::new(cache);

    let mut persisting_header = BlockHeader::default();
    persisting_header.set_index(0);
    persisting_header.set_timestamp(1_700_000_000_000);
    let persisting_block = Some(Block::from_parts(persisting_header, vec![]));
    let mut engine = engine_for(Arc::clone(&snapshot), persisting_block, settings);
    engine
        .load_script(script, CallFlags::ALL, Some(self_hash))
        .expect("script loads");
    assert_eq!(
        engine.execute_allow_fault(),
        VmState::HALT,
        "destroy must HALT under Gorgon"
    );

    // Block-before-erase reaches the identical final state: record, index, and
    // storage gone; hash blocked; whitelist cleaned.
    assert!(
        snapshot
            .get(&ContractManagement::contract_storage_key(&self_hash))
            .is_none(),
        "contract record deleted"
    );
    assert!(snapshot.get(&index_key).is_none(), "id->hash index deleted");
    assert!(
        snapshot.get(&user_row).is_none(),
        "contract storage deleted"
    );
    assert!(
        snapshot
            .get(&crate::PolicyContract::blocked_account_key(&self_hash))
            .is_some(),
        "destroyed contract is blocked"
    );
    assert!(snapshot.get(&wl_key).is_none(), "whitelist entry deleted");
    assert!(
        engine
            .notifications()
            .iter()
            .any(|n| n.event_name == "Destroy"),
        "Destroy event emitted"
    );
}

#[test]
fn destroy_is_a_noop_for_a_non_contract_caller() {
    let cache = DataCache::new(false);
    put_contract_record(
        &cache,
        &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
    );
    let script = destroy_script();
    let self_hash = UInt160::from_script(&script);
    let snapshot = Arc::new(cache);

    // No contract record for the calling script: C# `if (contract is null)
    // return;` — a successful no-op that writes nothing.
    let mut engine = engine_for(Arc::clone(&snapshot), None, ProtocolSettings::default());
    engine
        .load_script(script, CallFlags::ALL, Some(self_hash))
        .expect("script loads");
    assert_eq!(
        engine.execute_allow_fault(),
        VmState::HALT,
        "no-op destroy HALTs"
    );
    assert!(
        snapshot
            .get(&crate::PolicyContract::blocked_account_key(&self_hash))
            .is_none(),
        "no blocked-account entry for a no-op destroy"
    );
    assert!(
        engine
            .notifications()
            .iter()
            .all(|n| n.event_name != "Destroy"),
        "no Destroy event for a no-op destroy"
    );
}

#[test]
fn block_account_internal_faun_writes_timestamp_and_is_idempotent() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfFaun, 0);
    let mut header = BlockHeader::default();
    header.set_index(1);
    header.set_timestamp(1_700_000_123_456);
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = engine_for(
        Arc::clone(&snapshot),
        Some(Block::from_parts(header, vec![])),
        settings,
    );

    let account = UInt160::from_bytes(&[0x33u8; 20]).unwrap();
    // First block: post-Faun the entry stores GetTime() (the persisting
    // block's timestamp) for Policy's recoverFund.
    assert!(
        crate::PolicyContract::new()
            .block_account_internal(&mut engine, &account)
            .unwrap()
    );
    let item = snapshot
        .get(&crate::PolicyContract::blocked_account_key(&account))
        .expect("blocked entry written");
    assert_eq!(
        BigInt::from_signed_bytes_le(&item.value_bytes()),
        BigInt::from(1_700_000_123_456i64)
    );
    // Already blocked -> false, nothing rewritten (C# returns early).
    assert!(
        !crate::PolicyContract::new()
            .block_account_internal(&mut engine, &account)
            .unwrap()
    );
}

#[test]
fn block_account_internal_rejects_native_hashes() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = engine_for(Arc::clone(&snapshot), None, ProtocolSettings::default());
    // C#: "Cannot block a native contract."
    let neo_hash = *crate::hashes::NEO_TOKEN_HASH;
    let err = crate::PolicyContract::new()
        .block_account_internal(&mut engine, &neo_hash)
        .unwrap_err();
    assert!(err.to_string().contains("native"));
    assert!(
        snapshot
            .get(&crate::PolicyContract::blocked_account_key(&neo_hash))
            .is_none()
    );
}

#[test]
fn block_account_internal_faun_runs_vote_transition_for_neo_holders() {
    // C# BlockAccountInternal post-Faun runs NEO.VoteInternal(account,
    // null): for a NEO-holding account the full vote transition executes
    // (here a no-op un-vote — the account votes for nobody), then the
    // blocked entry is written with the persisting block's timestamp.
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfFaun, 0);
    let mut header = BlockHeader::default();
    header.set_index(1);
    header.set_timestamp(1_700_000_000_000);
    let cache = DataCache::new(false);
    let account = UInt160::from_bytes(&[0x44u8; 20]).unwrap();
    // Seed a NeoToken account state holding 100 NEO.
    let neo_key = crate::NeoToken::account_key(&account);
    let neo_state = BinarySerializer::serialize(
        &StackItem::from_struct(vec![
            StackItem::from_int(100),
            StackItem::from_int(0),
            StackItem::Null,
            StackItem::from_int(0),
        ]),
        &ExecutionEngineLimits::default(),
    )
    .unwrap();
    cache.add(neo_key, StorageItem::from_bytes(neo_state));
    let snapshot = Arc::new(cache);
    let mut engine = engine_for(
        Arc::clone(&snapshot),
        Some(Block::from_parts(header, vec![])),
        settings,
    );

    assert!(
        crate::PolicyContract::new()
            .block_account_internal(&mut engine, &account)
            .unwrap()
    );
    let item = snapshot
        .get(&crate::PolicyContract::blocked_account_key(&account))
        .expect("blocked entry written after the vote transition");
    assert_eq!(
        BigInt::from_signed_bytes_le(&item.value_bytes()),
        BigInt::from(1_700_000_000_000i64),
        "entry stores GetTime() for recoverFund"
    );
}
