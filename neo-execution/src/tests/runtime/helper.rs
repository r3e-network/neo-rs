use super::*;
use neo_payloads::{Block, Header, OracleResponse, Signer, Transaction, TransactionAttribute};
use neo_primitives::{OracleResponseCode, WitnessScope};

#[test]
fn verify_witnesses_uses_transaction_witnesses_for_count_check() {
    let witness = Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSH1.byte()]);
    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_signers(vec![Signer::new(witness.script_hash(), WitnessScope::NONE)]);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_witnesses(vec![witness]);

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    assert!(
        Helper::verify_witnesses(&tx, &settings, &snapshot, Helper::MAX_VERIFICATION_GAS),
        "transactions must expose their witnesses before the engine executes, matching C# Transaction.Witnesses"
    );
}

#[test]
fn verify_witnesses_uses_genesis_header_witnesses() {
    let witness = Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSH1.byte()]);
    let mut header = Header::new();
    header.set_prev_hash(UInt256::zero());
    header.witness = witness;

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    assert!(
        Helper::verify_witnesses(&header, &settings, &snapshot, Helper::MAX_VERIFICATION_GAS),
        "C# Header.IVerifiable exposes exactly one witness and uses Witness.ScriptHash for genesis headers"
    );
}

#[test]
fn verify_witnesses_uses_genesis_block_header_witnesses() {
    let witness = Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSH1.byte()]);
    let mut block = Block::new();
    block.header.set_prev_hash(UInt256::zero());
    block.header.witness = witness;

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    assert!(
        Helper::verify_witnesses(&block, &settings, &snapshot, Helper::MAX_VERIFICATION_GAS),
        "C# Block.IVerifiable delegates witnesses and verifying hashes to Header"
    );
}

#[test]
fn verify_witness_uses_verifiable_container_hook() {
    let source = include_str!("../../runtime/helper.rs");
    let start = source
        .find("pub fn verify_witness<V: VerifiableExt>")
        .expect("verify_witness function exists");
    let end = source[start..]
        .find("let mut engine = ApplicationEngine::new")
        .map(|offset| start + offset)
        .expect("engine construction exists");
    let setup = &source[start..end];

    assert!(
        setup.contains("to_verifiable_container()"),
        "Helper.VerifyWitness should install the actual C# IVerifiable-equivalent payload through VerifiableExt"
    );
    assert!(
        !setup.contains("as_transaction()"),
        "verification container selection must not regress to a transaction-only special case"
    );
}

#[test]
fn verify_witness_uses_transaction_container_for_check_witness() {
    let delegated_signer = UInt160::parse("0x0102030405060708090a0b0c0d0e0f1011121314").unwrap();

    let mut builder = ScriptBuilder::new();
    builder.emit_push(&delegated_signer.to_array());
    builder
        .emit_syscall("System.Runtime.CheckWitness")
        .expect("CheckWitness syscall");
    let witness = Witness::new_with_scripts(Vec::new(), builder.to_array());

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_signers(vec![
        Signer::new(witness.script_hash(), WitnessScope::GLOBAL),
        Signer::new(delegated_signer, WitnessScope::GLOBAL),
    ]);
    tx.set_witnesses(vec![witness.clone(), Witness::empty()]);

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    Helper::verify_witness(
        &tx,
        &settings,
        &snapshot,
        &witness.script_hash(),
        &witness,
        Helper::MAX_VERIFICATION_GAS,
    )
    .expect(
        "CheckWitness inside transaction witness verification must see the real Transaction container",
    );
}

#[test]
fn verify_witness_uses_transaction_container_for_current_signers() {
    let second_signer = UInt160::parse("0x14131211100f0e0d0c0b0a090807060504030201").unwrap();

    let mut builder = ScriptBuilder::new();
    builder
        .emit_syscall("System.Runtime.CurrentSigners")
        .expect("CurrentSigners syscall");
    builder.emit_opcode(OpCode::SIZE);
    builder.emit_opcode(OpCode::PUSH2);
    builder.emit_opcode(OpCode::NUMEQUAL);
    let witness = Witness::new_with_scripts(Vec::new(), builder.to_array());

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_signers(vec![
        Signer::new(witness.script_hash(), WitnessScope::NONE),
        Signer::new(second_signer, WitnessScope::GLOBAL),
    ]);
    tx.set_witnesses(vec![witness.clone(), Witness::empty()]);

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    Helper::verify_witness(
        &tx,
        &settings,
        &snapshot,
        &witness.script_hash(),
        &witness,
        Helper::MAX_VERIFICATION_GAS,
    )
    .expect(
        "CurrentSigners inside transaction witness verification must see the real Transaction container",
    );
}

#[test]
fn verify_witness_uses_transaction_container_for_get_script_container() {
    let mut builder = ScriptBuilder::new();
    builder
        .emit_syscall("System.Runtime.GetScriptContainer")
        .expect("GetScriptContainer syscall");
    builder.emit_push_int(2);
    builder.emit_opcode(OpCode::PICKITEM);
    builder.emit_push_int(0x0102_0304);
    builder.emit_opcode(OpCode::NUMEQUAL);
    let witness = Witness::new_with_scripts(Vec::new(), builder.to_array());

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_signers(vec![Signer::new(witness.script_hash(), WitnessScope::NONE)]);
    tx.set_witnesses(vec![witness.clone()]);

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    Helper::verify_witness(
        &tx,
        &settings,
        &snapshot,
        &witness.script_hash(),
        &witness,
        Helper::MAX_VERIFICATION_GAS,
    )
    .expect("GetScriptContainer inside transaction witness verification must expose the real Transaction");
}

#[test]
fn oracle_response_check_witness_faults_when_request_is_missing() {
    let delegated_signer = UInt160::parse("0x0102030405060708090a0b0c0d0e0f1011121314").unwrap();

    let mut builder = ScriptBuilder::new();
    builder.emit_push(&delegated_signer.to_array());
    builder
        .emit_syscall("System.Runtime.CheckWitness")
        .expect("CheckWitness syscall");
    builder.emit_opcode(OpCode::NOT);
    let witness = Witness::new_with_scripts(Vec::new(), builder.to_array());

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_attributes(vec![TransactionAttribute::OracleResponse(
        OracleResponse::new(7, OracleResponseCode::Success, Vec::new()),
    )]);
    tx.set_signers(vec![Signer::new(witness.script_hash(), WitnessScope::NONE)]);
    tx.set_witnesses(vec![witness.clone()]);

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    assert!(
        Helper::verify_witness(
            &tx,
            &settings,
            &snapshot,
            &witness.script_hash(),
            &witness,
            Helper::MAX_VERIFICATION_GAS,
        )
        .is_err(),
        "C# CheckWitnessInternal faults when an OracleResponse request lookup is missing"
    );
}

#[test]
fn verify_witness_rejects_strictly_invalid_verification_script_before_execution() {
    let verification_script = vec![
        OpCode::PUSH1.byte(),
        OpCode::RET.byte(),
        OpCode::JMP.byte(),
        0x7f,
    ];
    let witness = Witness::new_with_scripts(Vec::new(), verification_script);

    let mut tx = Transaction::new();
    tx.set_version(0);
    tx.set_nonce(0x0102_0304);
    tx.set_system_fee(1);
    tx.set_network_fee(1);
    tx.set_valid_until_block(42);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_signers(vec![Signer::new(witness.script_hash(), WitnessScope::NONE)]);
    tx.set_witnesses(vec![witness.clone()]);

    let settings = ProtocolSettings::default();
    let snapshot = DataCache::new(false);

    assert!(
        Helper::verify_witness(
            &tx,
            &settings,
            &snapshot,
            &witness.script_hash(),
            &witness,
            Helper::MAX_VERIFICATION_GAS,
        )
        .is_err(),
        "C# Helper.VerifyWitness constructs Script(verification, strict: true) before execution"
    );
}
