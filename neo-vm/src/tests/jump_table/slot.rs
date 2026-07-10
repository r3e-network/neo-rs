use super::*;
use crate::script::Script;

fn engine() -> ExecutionEngine {
    let mut engine = ExecutionEngine::<()>::new(None);
    engine
        .load_script(Script::new_relaxed(vec![OpCode::RET.byte()]), -1, 0)
        .expect("load test script");
    engine
}

/// C# `InitSSlot` faults on a zero operand (a zero-sized static-field slot is
/// meaningless) after the twice-guard. JumpTable.Slot.cs:31-33.
#[test]
fn initsslot_zero_operand_faults_like_csharp() {
    let mut engine = engine();
    assert!(
        init_static_slot(&mut engine, &Instruction::new(OpCode::INITSSLOT, &[0])).is_err(),
        "INITSSLOT 0 must fault (C# InitSSlot throws on a zero operand)"
    );
}

#[test]
fn initsslot_nonzero_operand_succeeds() {
    let mut engine = engine();
    init_static_slot(&mut engine, &Instruction::new(OpCode::INITSSLOT, &[3]))
        .expect("INITSSLOT 3 succeeds");
}
