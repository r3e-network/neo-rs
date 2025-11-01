//! Targeted parity tests covering recently restored VM functionality.

use neo_vm::{script::Script, script_builder::ScriptBuilder, stack_item::StackItem};
use sha2::{Digest, Sha256};
use std::sync::Arc;

/// Helper that computes the C#-style syscall hash directly.
fn double_sha256(bytes: &[u8]) -> [u8; 32] {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    let first = hasher.finalize();

    let mut hasher = Sha256::new();
    hasher.update(first);
    let digest = hasher.finalize();

    let mut result = [0u8; 32];
    result.copy_from_slice(&digest);
    result
}

#[test]
fn script_builder_syscall_hash_matches_csharp() {
    let api = "System.Runtime.Log";
    let expected = double_sha256(api.as_bytes());
    let expected_prefix = u32::from_le_bytes([expected[0], expected[1], expected[2], expected[3]]);

    let computed = ScriptBuilder::hash_syscall(api).expect("hash_syscall should succeed");
    assert_eq!(computed, expected_prefix);
}

#[test]
fn script_builder_emit_syscall_writes_hash() {
    let mut builder = ScriptBuilder::new();
    let api = "System.Runtime.GetTime";
    builder
        .emit_syscall(api)
        .expect("emit_syscall should succeed");

    let script_bytes = builder.to_array();
    assert_eq!(script_bytes.len(), 5, "SYSCALL opcode + 4-byte hash");
    assert_eq!(script_bytes[0], neo_vm::op_code::OpCode::SYSCALL as u8);

    let expected_hash = ScriptBuilder::hash_syscall(api).unwrap().to_le_bytes();
    assert_eq!(&script_bytes[1..5], &expected_hash);
}

#[test]
fn stack_item_pointer_matches_script_and_position() {
    let script_a = Arc::new(Script::new_relaxed(vec![0x01, 0x02]));
    let script_b = Arc::new(Script::new_relaxed(vec![0xFF]));

    let ptr_a1 = StackItem::from_pointer(Arc::clone(&script_a), 123);
    let ptr_a2 = StackItem::from_pointer(Arc::clone(&script_a), 123);
    let ptr_a3 = StackItem::from_pointer(Arc::clone(&script_a), 456);
    let ptr_b = StackItem::from_pointer(script_b, 123);

    assert_eq!(
        ptr_a1, ptr_a2,
        "pointers with same script and position should match"
    );
    assert_ne!(ptr_a1, ptr_a3, "different positions should differ");
    assert_ne!(ptr_a1, ptr_b, "different scripts should differ");

    let pointer = ptr_a1
        .get_pointer()
        .expect("pointer extraction should succeed");
    assert_eq!(pointer.position(), 123);
    assert!(Arc::ptr_eq(&pointer.script_arc(), &script_a));
}
