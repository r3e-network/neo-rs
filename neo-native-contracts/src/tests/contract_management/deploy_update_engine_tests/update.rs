use super::fixtures::*;
use crate::contract_management::ContractManagement;
use neo_config::ProtocolSettings;
use neo_execution::ContractState;
use neo_execution::native_contract::build_native_contract_state;
use neo_manifest::NefFile;
use neo_primitives::{CallFlags, UInt160};
use neo_storage::StorageItem;
use neo_storage::persistence::DataCache;
use neo_vm_rs::{OpCode, VmState};
use std::sync::Arc;

#[test]
fn update_bumps_counter_swaps_payloads_and_notifies() {
    crate::install();
    let cache = DataCache::new(false);
    put_contract_record(
        &cache,
        &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
    );

    // The entry script IS the updating contract: pin its hash and seed its
    // record (id 7) plus the id index entry.
    let new_nef = NefFile::new("updated-compiler".to_string(), vec![OpCode::RET.byte()]);
    let new_manifest = deployable_manifest("SelfUpdateFixture");
    let script = update_script(
        Some(&new_nef.to_bytes()),
        Some(&manifest_json(&new_manifest)),
        CallFlags::ALL,
    );
    let self_hash = UInt160::from_script(&script);
    let fixture = ContractState::new(
        7,
        self_hash,
        minimal_nef(),
        deployable_manifest("SelfUpdateFixture"),
    );
    put_contract_record(&cache, &fixture);
    let index_key = ContractManagement::contract_id_storage_key(7);
    cache.add(
        index_key.clone(),
        StorageItem::from_bytes(self_hash.to_bytes().to_vec()),
    );
    let snapshot = Arc::new(cache);

    let (state, engine) = run_update(&snapshot, script, self_hash);
    assert_eq!(state, VmState::HALT, "update must HALT");

    // Same id + hash, UpdateCounter bumped, NEF and manifest swapped.
    let updated = ContractManagement::get_contract_from_snapshot(&snapshot, &self_hash)
        .unwrap()
        .expect("updated record exists");
    assert_eq!(updated.id, 7, "id is preserved");
    assert_eq!(updated.hash, self_hash, "hash is preserved");
    assert_eq!(updated.update_counter, 1, "UpdateCounter bumped");
    assert_eq!(updated.nef.compiler, "updated-compiler");
    assert_eq!(updated.nef.checksum, new_nef.checksum);
    assert_eq!(updated.manifest.name, "SelfUpdateFixture");
    // The id index entry is untouched.
    assert_eq!(
        snapshot
            .get(&index_key)
            .expect("index intact")
            .value_bytes()
            .to_vec(),
        self_hash.to_bytes().to_vec()
    );

    // The storage fee on the payload was charged (no minimum-fee floor).
    let payload_len = (new_nef.to_bytes().len() + manifest_json(&new_manifest).len()) as i64;
    assert!(engine.fee_consumed() >= i64::from(engine.storage_price()) * payload_len);

    // The Update notification carries the contract hash.
    let notifications = engine.notifications();
    let update_event = notifications
        .iter()
        .find(|n| n.event_name == "Update")
        .expect("Update event emitted");
    assert_eq!(update_event.script_hash, ContractManagement::script_hash());
    assert_eq!(
        update_event.state[0].as_bytes().unwrap(),
        self_hash.to_bytes().to_vec()
    );
}

#[test]
fn update_with_null_nef_keeps_the_old_nef() {
    crate::install();
    let cache = DataCache::new(false);
    put_contract_record(
        &cache,
        &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
    );

    // update(null, manifest): only the manifest changes (one extra
    // supported standard); the NEF stays byte-identical.
    let mut new_manifest = deployable_manifest("NullNefFixture");
    new_manifest.supported_standards = vec!["NEP-17".to_string()];
    let script = update_script(None, Some(&manifest_json(&new_manifest)), CallFlags::ALL);
    let self_hash = UInt160::from_script(&script);
    let original_nef = minimal_nef();
    let fixture = ContractState::new(
        3,
        self_hash,
        original_nef.clone(),
        deployable_manifest("NullNefFixture"),
    );
    put_contract_record(&cache, &fixture);
    let snapshot = Arc::new(cache);

    let (state, _) = run_update(&snapshot, script, self_hash);
    assert_eq!(state, VmState::HALT, "manifest-only update must HALT");
    let updated = ContractManagement::get_contract_from_snapshot(&snapshot, &self_hash)
        .unwrap()
        .expect("record exists");
    assert_eq!(updated.update_counter, 1);
    assert_eq!(updated.nef.checksum, original_nef.checksum, "NEF unchanged");
    assert_eq!(updated.nef.compiler, original_nef.compiler);
    assert_eq!(
        updated.manifest.supported_standards,
        vec!["NEP-17".to_string()]
    );
}

#[test]
fn update_validation_failures_fault() {
    crate::install();

    // Both args null.
    {
        let cache = DataCache::new(false);
        put_contract_record(
            &cache,
            &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
        );
        let script = update_script(None, None, CallFlags::ALL);
        let self_hash = UInt160::from_script(&script);
        put_contract_record(
            &cache,
            &ContractState::new(4, self_hash, minimal_nef(), deployable_manifest("BothNull")),
        );
        let (state, _) = run_update(&Arc::new(cache), script, self_hash);
        assert_eq!(state, VmState::FAULT, "null nef + null manifest must fault");
    }

    // The caller has no contract record.
    {
        let cache = DataCache::new(false);
        put_contract_record(
            &cache,
            &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
        );
        let script = update_script(
            Some(&minimal_nef().to_bytes()),
            Some(&manifest_json(&deployable_manifest("NoRecord"))),
            CallFlags::ALL,
        );
        let self_hash = UInt160::from_script(&script);
        let (state, _) = run_update(&Arc::new(cache), script, self_hash);
        assert_eq!(state, VmState::FAULT, "non-contract caller must fault");
    }

    // The manifest name cannot change.
    {
        let cache = DataCache::new(false);
        put_contract_record(
            &cache,
            &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
        );
        let script = update_script(
            None,
            Some(&manifest_json(&deployable_manifest("RenamedFixture"))),
            CallFlags::ALL,
        );
        let self_hash = UInt160::from_script(&script);
        put_contract_record(
            &cache,
            &ContractState::new(
                5,
                self_hash,
                minimal_nef(),
                deployable_manifest("OriginalFixture"),
            ),
        );
        let snapshot = Arc::new(cache);
        let (state, _) = run_update(&snapshot, script, self_hash);
        assert_eq!(state, VmState::FAULT, "renaming must fault");
        // The seeded record is untouched (the name check precedes writes).
        let unchanged = ContractManagement::get_contract_from_snapshot(&snapshot, &self_hash)
            .unwrap()
            .expect("record still present");
        assert_eq!(unchanged.manifest.name, "OriginalFixture");
        assert_eq!(unchanged.update_counter, 0);
    }

    // The update counter is saturated at u16::MAX.
    {
        let cache = DataCache::new(false);
        put_contract_record(
            &cache,
            &build_native_contract_state(&ContractManagement, &ProtocolSettings::default(), 0),
        );
        let script = update_script(
            Some(&minimal_nef().to_bytes()),
            Some(&manifest_json(&deployable_manifest("MaxedFixture"))),
            CallFlags::ALL,
        );
        let self_hash = UInt160::from_script(&script);
        let mut fixture = ContractState::new(
            6,
            self_hash,
            minimal_nef(),
            deployable_manifest("MaxedFixture"),
        );
        fixture.update_counter = u16::MAX;
        put_contract_record(&cache, &fixture);
        let (state, _) = run_update(&Arc::new(cache), script, self_hash);
        assert_eq!(state, VmState::FAULT, "maxed update counter must fault");
    }
}
