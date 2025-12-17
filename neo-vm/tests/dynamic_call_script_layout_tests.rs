use neo_vm::op_code::OpCode;
use neo_vm::script_builder::ScriptBuilder;

#[test]
fn dynamic_call_script_layout_matches_csharp_extensions() {
    // Mirrors C# Neo.Extensions.ScriptBuilderExtensions.EmitDynamicCall:
    // CreateArray(args), EmitPush(flags), EmitPush(method), EmitPush(scriptHash),
    // EmitSysCall(System.Contract.Call).
    let contract_hash = [0x11u8; 20];

    let syscall_hash = ScriptBuilder::hash_syscall("System.Contract.Call").expect("syscall hash");

    let mut builder = ScriptBuilder::new();

    // Empty args -> NEWARRAY0.
    builder.emit_opcode(OpCode::NEWARRAY0);
    // CallFlags.All == 0x0F which is PUSH15 (PUSH0 + 15).
    builder.emit_push_int(15);
    // Method string "symbol".
    builder.emit_push(b"symbol");
    // Contract hash bytes.
    builder.emit_push(&contract_hash);
    // Syscall.
    builder
        .emit_syscall("System.Contract.Call")
        .expect("emit syscall");

    let script = builder.to_array();

    let mut expected = vec![
        OpCode::NEWARRAY0 as u8,
        (OpCode::PUSH0 as u8) + 15,
        OpCode::PUSHDATA1 as u8,
        6,
    ];
    expected.extend_from_slice(b"symbol");
    expected.extend_from_slice(&[OpCode::PUSHDATA1 as u8, 20]);
    expected.extend_from_slice(&contract_hash);
    expected.push(OpCode::SYSCALL as u8);
    expected.extend_from_slice(&syscall_hash.to_le_bytes());

    assert_eq!(script, expected);
}
