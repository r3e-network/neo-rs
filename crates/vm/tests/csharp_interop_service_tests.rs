use neo_vm::op_code::OpCode;
use neo_vm::script_builder::ScriptBuilder;

#[test]
fn syscall_hash_matches_csharp_reference() {
    let hash = ScriptBuilder::hash_syscall("System.Runtime.Platform")
        .expect("hash computation should succeed");
    assert_eq!(hash, 0xf6fc79b2);
}

#[test]
fn emit_syscall_serialises_opcode_and_hash() {
    let mut builder = ScriptBuilder::new();
    builder
        .emit_syscall("System.Runtime.Platform")
        .expect("emit_syscall");
    let bytes = builder.to_array();

    assert_eq!(bytes.len(), 1 + 4);
    assert_eq!(bytes[0], OpCode::SYSCALL as u8);

    // Syscall operands are encoded little-endian.
    assert_eq!(&bytes[1..], &0xf6fc79b2u32.to_le_bytes());
}
