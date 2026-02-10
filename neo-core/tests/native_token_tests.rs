use neo_core::network::p2p::payloads::{signer::Signer, transaction::Transaction};
use neo_core::persistence::DataCache;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::binary_serializer::BinarySerializer;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract_parameter_type::ContractParameterType;
use neo_core::smart_contract::contract_state::{ContractState, NefFile};
use neo_core::smart_contract::manifest::{
    ContractAbi, ContractManifest, ContractMethodDescriptor, ContractParameterDefinition,
};
use neo_core::smart_contract::native::{ContractManagement, GasToken, NativeContract, NeoToken};
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::smart_contract::IInteroperable;
use neo_core::wallets::KeyPair;
use neo_core::witness::Witness;
use neo_core::{IVerifiable, UInt160, WitnessScope};
use neo_vm::{ExecutionEngineLimits, OpCode, ScriptBuilder};
use num_bigint::BigInt;
use num_traits::Zero;
use std::sync::Arc;

#[test]
fn neo_token_hash_matches_reference() {
    let neo = NeoToken::new();
    assert_eq!(
        neo.hash().to_hex_string(),
        "0xef4073a0f2b305a38ec4050e4d3d28bc40ea63f5"
    );
    assert_eq!(neo.symbol(), "NEO");
    assert_eq!(neo.decimals(), 0);
}

#[test]
fn gas_token_hash_matches_reference() {
    let gas = GasToken::new();
    assert_eq!(
        gas.hash().to_hex_string(),
        "0xd2a4cff31913016155e38e474a2c06d08be276cf"
    );
    assert_eq!(gas.symbol(), "GAS");
    assert_eq!(gas.decimals(), 8);
}

#[test]
fn system_contract_call_can_invoke_native_gas_symbol() {
    let snapshot = Arc::new(DataCache::new(false));
    let signer = sample_account(0xAA);
    let mut engine = make_engine(Arc::clone(&snapshot), signer);

    let gas = GasToken::new();
    let state = gas
        .contract_state(engine.protocol_settings(), 0)
        .expect("native contract state");
    let symbol_descriptor = state
        .manifest
        .abi
        .methods
        .iter()
        .find(|m| m.name == "symbol")
        .expect("symbol method descriptor");
    assert_eq!(
        state.nef.script[symbol_descriptor.offset as usize],
        OpCode::PUSH0 as u8
    );
    let mut sb = ScriptBuilder::new();

    // C# parity: ScriptBuilderExtensions.EmitDynamicCall(scriptHash, method, CallFlags.All, args)
    // stack before SYSCALL: [args_array, call_flags, method, contract_hash]
    sb.emit_opcode(OpCode::NEWARRAY0);
    sb.emit_push_int(CallFlags::ALL.bits() as i64);
    sb.emit_push_string("symbol");
    sb.emit_push_byte_array(&gas.hash().to_bytes());
    sb.emit_syscall("System.Contract.Call")
        .expect("System.Contract.Call syscall");
    sb.emit_opcode(OpCode::RET);

    engine
        .load_script(sb.to_array(), CallFlags::ALL, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.result_stack().len(), 1);
    let symbol_bytes = engine
        .result_stack()
        .peek(0)
        .expect("result")
        .as_bytes()
        .expect("bytes");
    assert_eq!(symbol_bytes, b"GAS");
}

fn make_engine(snapshot: Arc<DataCache>, signer: UInt160) -> ApplicationEngine {
    const TEST_GAS_LIMIT: i64 = 400_000_000;
    let mut container = Transaction::new();
    container.set_signers(vec![Signer::new(signer, WitnessScope::GLOBAL)]);
    // Add a witness for the signer to pass check_witness validation
    container.add_witness(Witness::new());
    let script_container: Arc<dyn IVerifiable> = Arc::new(container);
    ApplicationEngine::new(
        TriggerType::Application,
        Some(script_container),
        snapshot,
        None,
        Default::default(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine")
}

fn sample_account(tag: u8) -> UInt160 {
    let bytes = [tag; 20];
    UInt160::from_bytes(&bytes).unwrap()
}

#[test]
fn gas_token_mint_burn_and_transfer_update_balances() {
    let snapshot = Arc::new(DataCache::new(false));
    let context_engine_snapshot = Arc::clone(&snapshot);
    let gas = GasToken::new();
    let account_a = sample_account(0xAA);
    let account_b = sample_account(0xBB);
    let amount = BigInt::from(1_000_000);

    let mut engine = make_engine(context_engine_snapshot, account_a);
    engine.set_current_script_hash(Some(gas.hash()));

    gas.mint(&mut engine, &account_a, &amount, false)
        .expect("mint succeeds");

    let balance_a = gas.balance_of_snapshot(snapshot.as_ref(), &account_a);
    assert_eq!(balance_a, amount);
    let balance_b = gas.balance_of_snapshot(snapshot.as_ref(), &account_b);
    assert!(balance_b.is_zero());

    // transfer half to account_b
    let transfer_bytes = amount.clone().to_signed_bytes_le();
    let from_bytes = account_a.to_bytes();
    let to_bytes = account_b.to_bytes();
    let transfer_args = vec![from_bytes, to_bytes, transfer_bytes.clone(), Vec::new()];
    let transfer_result = gas
        .invoke(&mut engine, "transfer", &transfer_args)
        .expect("transfer call");
    assert_eq!(transfer_result, vec![1]);

    let balance_a_after = gas.balance_of_snapshot(snapshot.as_ref(), &account_a);
    let balance_b_after = gas.balance_of_snapshot(snapshot.as_ref(), &account_b);
    assert!(balance_a_after < balance_a);
    assert_eq!(balance_b_after.clone() + balance_a_after.clone(), amount);

    gas.burn(&mut engine, &account_a, &balance_a_after)
        .expect("burn succeeds");
    assert!(gas
        .balance_of_snapshot(snapshot.as_ref(), &account_a)
        .is_zero());
}

#[test]
fn gas_transfer_triggers_on_nep17_payment_with_native_caller() {
    let snapshot = Arc::new(DataCache::new(false));
    let sender = sample_account(0xAA);
    // Use a large gas limit since ContractManagement.deploy charges storage fees.
    let mut container = Transaction::new();
    container.set_signers(vec![Signer::new(sender, WitnessScope::GLOBAL)]);
    container.add_witness(Witness::new());
    let script_container: Arc<dyn IVerifiable> = Arc::new(container);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(script_container),
        Arc::clone(&snapshot),
        None,
        Default::default(),
        50_000_000_000,
        None,
    )
    .expect("engine");

    // Build a simple contract script that emits a Notify("Payment", [callingHash]).
    let mut sb = ScriptBuilder::new();
    sb.emit_push_string("Payment");
    sb.emit_syscall("System.Runtime.GetCallingScriptHash")
        .expect("syscall hash");
    sb.emit_push_int(1);
    sb.emit_pack();
    sb.emit_syscall("System.Runtime.Notify")
        .expect("notify syscall");
    let script = sb.to_array();

    // Create NEF and manifest with onNEP17Payment entry.
    let nef = NefFile::new("test".to_string(), script);
    let on_payment = ContractMethodDescriptor::new(
        "onNEP17Payment".to_string(),
        vec![
            ContractParameterDefinition::new("from".to_string(), ContractParameterType::Hash160)
                .unwrap(),
            ContractParameterDefinition::new("amount".to_string(), ContractParameterType::Integer)
                .unwrap(),
            ContractParameterDefinition::new("data".to_string(), ContractParameterType::Any)
                .unwrap(),
        ],
        ContractParameterType::Void,
        0,
        false,
    )
    .unwrap();

    let mut manifest = ContractManifest::new("PaymentReceiver".to_string());
    manifest.abi = ContractAbi::new(vec![on_payment], vec![]);

    let manifest_json = manifest.to_json().expect("manifest json");
    let manifest_bytes = serde_json::to_vec(&manifest_json).expect("serialize manifest");

    // Deploy through native ContractManagement so the engine can fetch it.
    let cm_hash = ContractManagement::new().hash();
    let deploy_args = vec![nef.to_bytes(), manifest_bytes, Vec::new()];
    let contract_bytes = engine
        .call_native_contract(cm_hash, "deploy", &deploy_args)
        .expect("deploy succeeds");
    let contract_item =
        BinarySerializer::deserialize(&contract_bytes, &ExecutionEngineLimits::default(), None)
            .expect("contract state item");
    let mut receiver = ContractState::new(
        0,
        UInt160::zero(),
        nef.clone(),
        ContractManifest::new(String::new()),
    );
    let _ = receiver.from_stack_item(contract_item);
    let receiver_hash = receiver.hash;

    // Fund sender with GAS and transfer to receiver contract.
    let gas = GasToken::new();
    engine.set_current_script_hash(Some(gas.hash()));
    let amount = BigInt::from(1_000_000);
    gas.mint(&mut engine, &sender, &amount, false)
        .expect("mint succeeds");

    let transfer_args = vec![
        sender.to_bytes(),
        receiver_hash.to_bytes(),
        amount.to_signed_bytes_le(),
        Vec::new(), // data = null
    ];
    let transfer_result = gas
        .invoke(&mut engine, "transfer", &transfer_args)
        .expect("transfer call");
    assert_eq!(transfer_result, vec![1]);

    // Load a dummy context so we can execute queued callbacks.
    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, None)
        .expect("load dummy");
    engine
        .process_pending_native_calls()
        .expect("queue processing");
    engine.execute().expect("execute callbacks");

    let payment_events: Vec<_> = engine
        .notifications()
        .iter()
        .filter(|n| n.event_name == "Payment")
        .collect();
    assert_eq!(payment_events.len(), 1, "expected one Payment event");
    let calling_hash_bytes = payment_events[0].state[0]
        .as_bytes()
        .expect("calling hash bytes");
    assert_eq!(calling_hash_bytes, gas.hash().to_bytes());
}

#[test]
fn neo_get_candidate_vote_returns_negative_one_for_missing_candidate() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        Default::default(),
        400_000_000,
        None,
    )
    .expect("engine");

    let key_pair = KeyPair::new(vec![1u8; 32]).expect("keypair");
    let pubkey = key_pair.compressed_public_key();

    let neo = NeoToken::new();
    let result = neo
        .invoke(&mut engine, "getCandidateVote", &[pubkey])
        .expect("getCandidateVote");
    let value = BigInt::from_signed_bytes_le(&result);
    assert_eq!(value, BigInt::from(-1));
}
