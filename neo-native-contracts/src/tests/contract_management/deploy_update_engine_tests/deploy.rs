use super::super::storage_key_int;
use super::fixtures::*;
use crate::contract_management::{ContractManagement, DEFAULT_MINIMUM_DEPLOYMENT_FEE};
use neo_config::{Hardfork, ProtocolSettings};
use neo_execution::helper::Helper;
use neo_manifest::{ContractMethodDescriptor, ContractParameterDefinition, NefFile};
use neo_primitives::{CallFlags, ContractParameterType, UInt160};
use neo_storage::persistence::SeekDirection;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::{OpCode, VmState};
use num_bigint::BigInt;

#[test]
fn deploy_writes_record_and_index_charges_fee_and_notifies() {
    let snapshot = seeded_snapshot();
    let sender = UInt160::from_bytes(&SENDER).unwrap();
    let nef = minimal_nef();
    let manifest = deployable_manifest("DeployFixture");

    let (state, engine) = run_deploy(
        &snapshot,
        ProtocolSettings::default(),
        sender,
        &nef.to_bytes(),
        &manifest_json(&manifest),
        None,
        CallFlags::ALL,
    );
    assert_eq!(state, VmState::HALT, "deploy must HALT");

    // The record lands at GetContractHash(sender, nef.CheckSum, name) and
    // round-trips through the shared reader.
    let expected_hash = Helper::get_contract_hash(&sender, nef.checksum, "DeployFixture");
    let deployed = ContractManagement::get_contract_from_snapshot(&snapshot, &expected_hash)
        .unwrap()
        .expect("deployed record exists");
    assert_eq!(
        deployed.id, 1,
        "first user contract takes the genesis next-id"
    );
    assert_eq!(deployed.update_counter, 0);
    assert_eq!(deployed.hash, expected_hash);
    assert_eq!(deployed.nef.checksum, nef.checksum);
    assert_eq!(deployed.manifest.name, "DeployFixture");

    // The big-endian id -> hash index entry.
    let index = snapshot
        .get(&ContractManagement::contract_id_storage_key(1))
        .expect("id index entry written");
    assert_eq!(
        index.value_bytes().to_vec(),
        expected_hash.to_bytes().to_vec()
    );
    // The next-available-id counter advanced to 2.
    assert_eq!(
        storage_key_int(&snapshot, ContractManagement::next_available_id_key()),
        Some(BigInt::from(2))
    );

    // The 10-GAS minimum deployment fee dominates this tiny payload and
    // was charged (C# AddFee(max(StoragePrice * size, MinimumFee))).
    assert!(
        engine.fee_consumed() >= DEFAULT_MINIMUM_DEPLOYMENT_FEE,
        "deployment fee charged: {}",
        engine.fee_consumed()
    );

    // The Deploy notification carries the new hash.
    let notifications = engine.notifications();
    let deploy_event = notifications
        .iter()
        .find(|n| n.event_name == "Deploy")
        .expect("Deploy event emitted");
    assert_eq!(deploy_event.script_hash, ContractManagement::script_hash());
    assert_eq!(
        deploy_event.state()[0].as_bytes().unwrap(),
        expected_hash.to_bytes().to_vec()
    );

    // deploy returns the new ContractState as the 5-field Array.
    let result = engine.result_stack().peek(0).expect("deploy result");
    let StackItem::Array(items) = result else {
        panic!("deploy must return an Array, got {result:?}");
    };
    assert_eq!(items.items().len(), 5);
    assert_eq!(
        items.items()[2].as_bytes().unwrap(),
        expected_hash.to_bytes().to_vec(),
        "field 2 is the contract hash"
    );
}

#[test]
fn deploy_hash_is_deterministic_and_duplicates_fault() {
    let snapshot = seeded_snapshot();
    let sender = UInt160::from_bytes(&SENDER).unwrap();
    let nef = minimal_nef();
    let manifest = deployable_manifest("DeterministicFixture");
    let manifest_bytes = manifest_json(&manifest);

    let (first, _) = run_deploy(
        &snapshot,
        ProtocolSettings::default(),
        sender,
        &nef.to_bytes(),
        &manifest_bytes,
        None,
        CallFlags::ALL,
    );
    assert_eq!(first, VmState::HALT);

    // Same sender + NEF checksum + name -> the same hash, so the second
    // deploy hits "Contract Already Exists" and faults.
    let (duplicate, _) = run_deploy(
        &snapshot,
        ProtocolSettings::default(),
        sender,
        &nef.to_bytes(),
        &manifest_bytes,
        None,
        CallFlags::ALL,
    );
    assert_eq!(duplicate, VmState::FAULT, "duplicate deploy must fault");

    // A different manifest NAME moves the hash: deploys fresh with id 2.
    let renamed = deployable_manifest("DeterministicFixtureB");
    let (second, _) = run_deploy(
        &snapshot,
        ProtocolSettings::default(),
        sender,
        &nef.to_bytes(),
        &manifest_json(&renamed),
        None,
        CallFlags::ALL,
    );
    assert_eq!(second, VmState::HALT);
    let hash_a = Helper::get_contract_hash(&sender, nef.checksum, "DeterministicFixture");
    let hash_b = Helper::get_contract_hash(&sender, nef.checksum, "DeterministicFixtureB");
    assert_ne!(hash_a, hash_b);
    let second_state = ContractManagement::get_contract_from_snapshot(&snapshot, &hash_b)
        .unwrap()
        .expect("second contract deployed");
    assert_eq!(second_state.id, 2, "ids allocate sequentially");
}

#[test]
fn deploy_runs_the_declared_deploy_callback_with_data() {
    // The contract script: `main()` = RET at 0; `_deploy(data, update)` at
    // `deploy_offset` stores [0xEE] under key [0x77] in the contract's own
    // storage — observable proof the queued callback executed.
    let mut script = ScriptBuilder::new();
    script.emit_opcode(OpCode::RET);
    let deploy_offset = script.len() as i32;
    script.emit_instruction(OpCode::INITSLOT, &[0x00, 0x02]);
    script.emit_push(&[0xEE]); // value (deepest)
    script.emit_push(&[0x77]); // key
    script
        .emit_syscall("System.Storage.GetContext")
        .expect("GetContext");
    script.emit_syscall("System.Storage.Put").expect("Put");
    script.emit_opcode(OpCode::RET);
    let nef = NefFile::new("e2e-test".to_string(), script.to_array());

    let mut manifest = deployable_manifest("CallbackFixture");
    manifest.abi.methods.push(
        ContractMethodDescriptor::new(
            "_deploy".to_string(),
            vec![
                ContractParameterDefinition::new("data".to_string(), ContractParameterType::Any)
                    .unwrap(),
                ContractParameterDefinition::new(
                    "update".to_string(),
                    ContractParameterType::Boolean,
                )
                .unwrap(),
            ],
            ContractParameterType::Void,
            deploy_offset,
            false,
        )
        .expect("_deploy descriptor"),
    );

    let snapshot = seeded_snapshot();
    let sender = UInt160::from_bytes(&SENDER).unwrap();
    let (state, _) = run_deploy(
        &snapshot,
        ProtocolSettings::default(),
        sender,
        &nef.to_bytes(),
        &manifest_json(&manifest),
        Some(&[0xAB]), // deploy(nef, manifest, data) overload
        CallFlags::ALL,
    );
    assert_eq!(
        state,
        VmState::HALT,
        "deploy with _deploy callback must HALT"
    );

    // The callback wrote into the new contract's storage space (id 1).
    let row = snapshot
        .get(&StorageKey::new(1, vec![0x77]))
        .expect("_deploy callback wrote the marker row");
    assert_eq!(row.value_bytes().to_vec(), vec![0xEE]);
}

#[test]
fn deploy_callback_local_storage_syscalls_use_csharp_parameter_order() {
    // HF_Faun local storage syscalls follow the same reflection binder order
    // as C#: parameter 0 is on top of the stack. Local.Put(key, value) must
    // pop key before value; Local.Find(prefix, options) must pop prefix
    // before options.
    let mut script = ScriptBuilder::new();
    script.emit_opcode(OpCode::RET);
    let deploy_offset = script.len() as i32;
    script.emit_instruction(OpCode::INITSLOT, &[0x00, 0x02]);
    script.emit_push(&[0xEE]); // value (deeper)
    script.emit_push(&[0x77]); // key (top)
    script
        .emit_syscall("System.Storage.Local.Put")
        .expect("Local.Put");
    script.emit_push_int(0); // options (deeper)
    script.emit_push(&[0x77]); // prefix (top)
    script
        .emit_syscall("System.Storage.Local.Find")
        .expect("Local.Find");
    script.emit_opcode(OpCode::DROP);
    script.emit_opcode(OpCode::RET);
    let nef = NefFile::new("e2e-test".to_string(), script.to_array());

    let mut manifest = deployable_manifest("LocalStorageCallbackFixture");
    manifest.abi.methods.push(
        ContractMethodDescriptor::new(
            "_deploy".to_string(),
            vec![
                ContractParameterDefinition::new("data".to_string(), ContractParameterType::Any)
                    .unwrap(),
                ContractParameterDefinition::new(
                    "update".to_string(),
                    ContractParameterType::Boolean,
                )
                .unwrap(),
            ],
            ContractParameterType::Void,
            deploy_offset,
            false,
        )
        .expect("_deploy descriptor"),
    );

    let snapshot = seeded_snapshot();
    let sender = UInt160::from_bytes(&SENDER).unwrap();
    let (state, _) = run_deploy(
        &snapshot,
        faun_from_genesis_settings(),
        sender,
        &nef.to_bytes(),
        &manifest_json(&manifest),
        Some(&[0xAB]),
        CallFlags::ALL,
    );
    assert_eq!(
        state,
        VmState::HALT,
        "local storage callback must follow C# syscall parameter order"
    );

    let row = snapshot
        .get(&StorageKey::new(1, vec![0x77]))
        .expect("Local.Put wrote under the key argument");
    assert_eq!(row.value_bytes().to_vec(), vec![0xEE]);
    assert!(
        snapshot.get(&StorageKey::new(1, vec![0xEE])).is_none(),
        "Local.Put must not swap key and value"
    );
}

#[test]
fn deploy_skips_the_callback_when_not_declared() {
    // The minimal fixture declares no `_deploy`: C# OnDeployAsync skips
    // the call (md is null) but still emits Deploy. Nothing is written
    // into the new contract's storage space.
    let snapshot = seeded_snapshot();
    let sender = UInt160::from_bytes(&SENDER).unwrap();
    let (state, engine) = run_deploy(
        &snapshot,
        ProtocolSettings::default(),
        sender,
        &minimal_nef().to_bytes(),
        &manifest_json(&deployable_manifest("NoCallback")),
        Some(&[0xAB]),
        CallFlags::ALL,
    );
    assert_eq!(state, VmState::HALT);
    assert!(
        engine
            .notifications()
            .iter()
            .any(|n| n.event_name == "Deploy")
    );
    let contract_rows: Vec<_> = snapshot
        .find(
            Some(&StorageKey::new(1, Vec::new())),
            SeekDirection::Forward,
        )
        .collect();
    assert!(
        contract_rows.is_empty(),
        "no _deploy, no contract storage writes"
    );
}

#[test]
fn deploy_validation_failures_fault() {
    let sender = UInt160::from_bytes(&SENDER).unwrap();
    let nef = minimal_nef();
    let manifest_bytes = manifest_json(&deployable_manifest("FaultFixture"));

    // Empty NEF payload.
    let (state, _) = run_deploy(
        &seeded_snapshot(),
        ProtocolSettings::default(),
        sender,
        &[],
        &manifest_bytes,
        None,
        CallFlags::ALL,
    );
    assert_eq!(state, VmState::FAULT, "empty NEF must fault");

    // Empty manifest payload.
    let (state, _) = run_deploy(
        &seeded_snapshot(),
        ProtocolSettings::default(),
        sender,
        &nef.to_bytes(),
        &[],
        None,
        CallFlags::ALL,
    );
    assert_eq!(state, VmState::FAULT, "empty manifest must fault");

    // A corrupted NEF checksum.
    let mut corrupted = nef.to_bytes();
    let last = corrupted.len() - 1;
    corrupted[last] ^= 0xFF;
    let (state, _) = run_deploy(
        &seeded_snapshot(),
        ProtocolSettings::default(),
        sender,
        &corrupted,
        &manifest_bytes,
        None,
        CallFlags::ALL,
    );
    assert_eq!(state, VmState::FAULT, "bad NEF checksum must fault");

    // The target hash is Policy-blocked (C# "has been blocked").
    let snapshot = seeded_snapshot();
    let blocked_hash = Helper::get_contract_hash(&sender, nef.checksum, "FaultFixture");
    snapshot.add(
        crate::PolicyContract::blocked_account_key(&blocked_hash),
        StorageItem::from_bytes(Vec::new()),
    );
    let (state, _) = run_deploy(
        &snapshot,
        ProtocolSettings::default(),
        sender,
        &nef.to_bytes(),
        &manifest_bytes,
        None,
        CallFlags::ALL,
    );
    assert_eq!(state, VmState::FAULT, "blocked target hash must fault");
    assert!(
        ContractManagement::get_contract_from_snapshot(&snapshot, &blocked_hash)
            .unwrap()
            .is_none(),
        "no record written for a blocked deploy"
    );
}

#[test]
fn deploy_post_aspidochelone_requires_call_flags_all() {
    // Schedule HF_Aspidochelone from genesis: a deploy carrying only
    // States|AllowNotify (the method's minimum) must fault, while
    // CallFlags.All succeeds (C# refs #2653 / #2673).
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfAspidochelone, 0);
    let sender = UInt160::from_bytes(&SENDER).unwrap();
    let nef = minimal_nef();
    let manifest_bytes = manifest_json(&deployable_manifest("AspidoFixture"));

    let (restricted, _) = run_deploy(
        &seeded_snapshot(),
        settings.clone(),
        sender,
        &nef.to_bytes(),
        &manifest_bytes,
        None,
        CallFlags::STATES | CallFlags::ALLOW_NOTIFY,
    );
    assert_eq!(
        restricted,
        VmState::FAULT,
        "partial flags must fault post-Aspidochelone"
    );

    let (full, _) = run_deploy(
        &seeded_snapshot(),
        settings,
        sender,
        &nef.to_bytes(),
        &manifest_bytes,
        None,
        CallFlags::ALL,
    );
    assert_eq!(
        full,
        VmState::HALT,
        "CallFlags.All deploy succeeds post-Aspidochelone"
    );
}
