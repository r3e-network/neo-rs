use neo_core::builders::WitnessBuilder;
use neo_vm::op_code::OpCode;

#[test]
fn witness_builder_create_empty() {
    let _builder = WitnessBuilder::create_empty();
}

#[test]
fn witness_builder_adds_invocation_with_builder() {
    let witness = WitnessBuilder::create_empty()
        .add_invocation_with_builder(|sb| {
            sb.emit_opcode(OpCode::NOP);
            sb.emit_opcode(OpCode::NOP);
            sb.emit_opcode(OpCode::NOP);
        })
        .unwrap()
        .build();

    assert_eq!(witness.invocation_script(), &[0x21, 0x21, 0x21]);
}

#[test]
fn witness_builder_adds_invocation_bytes() {
    let witness = WitnessBuilder::create_empty()
        .add_invocation(vec![0x01, 0x02, 0x03])
        .unwrap()
        .build();

    assert_eq!(witness.invocation_script(), &[0x01, 0x02, 0x03]);
}

#[test]
fn witness_builder_adds_verification_with_builder() {
    let witness = WitnessBuilder::create_empty()
        .add_verification_with_builder(|sb| {
            sb.emit_opcode(OpCode::NOP);
            sb.emit_opcode(OpCode::NOP);
            sb.emit_opcode(OpCode::NOP);
        })
        .unwrap()
        .build();

    assert_eq!(witness.verification_script(), &[0x21, 0x21, 0x21]);
}

#[test]
fn witness_builder_adds_verification_bytes() {
    let witness = WitnessBuilder::create_empty()
        .add_verification(vec![0x01, 0x02, 0x03])
        .unwrap()
        .build();

    assert_eq!(witness.verification_script(), &[0x01, 0x02, 0x03]);
}
