use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::contract::Contract;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::UInt160;
use neo_vm::{OpCode, ScriptBuilder, VMState};
use std::sync::Arc;

fn emit_byte_array_array(builder: &mut ScriptBuilder, items: &[Vec<u8>]) {
    for item in items {
        builder.emit_push_byte_array(item);
    }
    builder.emit_push_int(items.len() as i64);
    builder.emit_opcode(OpCode::PACK);
}

#[test]
fn contract_create_standard_account_matches_redeem_script_hash() {
    let snapshot = Arc::new(DataCache::new(false));
    let settings = ProtocolSettings::default();
    let public_key = settings.standby_committee[0].clone();
    let expected = Contract::create_signature_contract(public_key.clone()).script_hash();
    let pubkey_bytes = public_key.encode_point(true).expect("pubkey bytes");

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        settings,
        1100_00000000,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    script.emit_push_byte_array(&pubkey_bytes);
    script
        .emit_syscall("System.Contract.CreateStandardAccount")
        .expect("syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.state(), VMState::HALT);
    let bytes = engine
        .result_stack()
        .peek(0)
        .expect("result item")
        .as_bytes()
        .expect("bytes");
    let account = UInt160::from_bytes(&bytes).expect("account");
    assert_eq!(account, expected);
}

#[test]
fn contract_create_multisig_account_matches_redeem_script_hash() {
    let snapshot = Arc::new(DataCache::new(false));
    let settings = ProtocolSettings::default();
    let public_keys = settings.standby_committee.iter().take(3).cloned().collect::<Vec<_>>();
    let expected = Contract::create_multi_sig_contract(2, &public_keys).script_hash();
    let pubkey_bytes = public_keys
        .iter()
        .map(|key| key.encode_point(true).expect("pubkey bytes"))
        .collect::<Vec<_>>();

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        snapshot,
        None,
        settings,
        1100_00000000,
        None,
    )
    .expect("engine");

    let mut script = ScriptBuilder::new();
    script.emit_push_int(2);
    emit_byte_array_array(&mut script, &pubkey_bytes);
    script
        .emit_syscall("System.Contract.CreateMultisigAccount")
        .expect("syscall");
    script.emit_opcode(OpCode::RET);

    engine
        .load_script(script.to_array(), CallFlags::NONE, None)
        .expect("load script");
    engine.execute().expect("execute");

    assert_eq!(engine.state(), VMState::HALT);
    let bytes = engine
        .result_stack()
        .peek(0)
        .expect("result item")
        .as_bytes()
        .expect("bytes");
    let account = UInt160::from_bytes(&bytes).expect("account");
    assert_eq!(account, expected);
}
