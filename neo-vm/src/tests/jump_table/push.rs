use super::*;
use crate::script::Script;

fn engine() -> ExecutionEngine {
    let mut engine = ExecutionEngine::<()>::new(None);
    engine
        .load_script(Script::new_relaxed(vec![OpCode::RET.byte()]), -1, 0)
        .expect("load test script");
    engine
}

/// C# `PushA` computes `checked(InstructionPointer + TokenI32)` (Push.cs:126),
/// faulting on i32 overflow before the bounds check. The Rust handler must fault
/// cleanly — never panic in a debug build nor wrap in release.
#[test]
fn pusha_address_overflow_faults_cleanly() {
    let mut engine = engine();
    let _ = engine
        .current_context_mut()
        .expect("current context")
        .set_instruction_pointer(1);

    // IP=1 plus an i32::MAX offset overflows the i32 address computation.
    let instr = Instruction::new(OpCode::PUSHA, &i32::MAX.to_le_bytes());
    assert!(
        push_a(&mut engine, &instr).is_err(),
        "PUSHA with an i32-overflowing address must fault, not panic"
    );
}
