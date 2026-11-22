//! The jump table that heights below `HF_Huyao` run (C# `NotHuyaoJumpTable`).
//!
//! Two behaviours are restored on top of the default table: the pre
//! neo-vm-PR-606 `SETITEM` and the pre-PR-575 `ROLL`. The `ROLL` special case
//! is consensus-visible: popping the single zero off a one-item stack faults
//! under the v3.11.2 in-place `Roll` but halts under the old behaviour.

use neo_vm::{ExecutionEngine, JumpTable, OpCode, Script, StackItem, VmState};

/// Runs `script` on an engine with the given jump table and returns the final
/// state plus the top of the result stack when the script halted.
fn run(script: Vec<u8>, jump_table: JumpTable) -> (VmState, Option<StackItem>) {
    let mut engine = ExecutionEngine::new(Some(jump_table));
    engine
        .load_script(Script::new_relaxed(script), -1, 0)
        .expect("load script");
    let state = engine.execute();
    let top = if state == VmState::HALT {
        engine.result_stack().peek(0).ok().map(|item| item.clone())
    } else {
        None
    };
    (state, top)
}

fn ops(opcodes: &[OpCode]) -> Vec<u8> {
    opcodes.iter().map(|opcode| opcode.byte()).collect()
}

/// Only `SETITEM` and `ROLL` differ between the default and the pre-Huyao
/// tables; every other handler stays shared (C# composes the table from the
/// default one and overrides exactly those two opcodes). Handler identity is
/// compared as addresses, mirroring the in-crate jump-table tests, since raw
/// function pointers do not implement meaningful equality.
#[test]
fn not_huyao_table_only_overrides_setitem_and_roll() {
    let default_table = JumpTable::default();
    let not_huyao = JumpTable::not_huyao();
    for opcode in OpCode::ALL {
        let default_address = default_table
            .get(opcode)
            .map(|handler| handler as *const () as usize)
            .unwrap_or(0);
        let not_huyao_address = not_huyao
            .get(opcode)
            .map(|handler| handler as *const () as usize)
            .unwrap_or(0);
        match opcode {
            OpCode::SETITEM | OpCode::ROLL => assert_ne!(
                default_address, not_huyao_address,
                "{opcode:?} must be overridden in the NotHuyao table"
            ),
            _ => assert_eq!(
                default_address, not_huyao_address,
                "{opcode:?} must not change"
            ),
        }
    }
}

/// `PUSH0 ROLL RET`: the v3.11.2 in-place `Roll` faults because the stack is
/// empty once the index is popped, the pre-PR-575 behaviour pops the single
/// zero and halts.
#[test]
fn roll_single_zero_halts_below_huyao_and_faults_on_the_default_table() {
    let script = ops(&[OpCode::PUSH0, OpCode::ROLL, OpCode::RET]);

    let (state, _) = run(script.clone(), JumpTable::default());
    assert_eq!(
        state,
        VmState::FAULT,
        "v3.11.2 Roll faults on the emptied stack"
    );

    let (state, top) = run(script, JumpTable::not_huyao());
    assert_eq!(state, VmState::HALT, "pre-PR-575 Roll pops the single zero");
    assert_eq!(top, None, "the popped zero leaves the stack empty");
}

/// A non-zero single item is not the special case: both tables fault when
/// `ROLL` empties the stack before the bounds check.
#[test]
fn roll_single_nonzero_item_faults_on_both_tables() {
    let script = ops(&[OpCode::PUSH1, OpCode::ROLL, OpCode::RET]);

    for table in [JumpTable::default(), JumpTable::not_huyao()] {
        let (state, _) = run(script.clone(), table);
        assert_eq!(state, VmState::FAULT);
    }
}

/// Everything except the single-zero special case delegates to the current
/// implementation, so both tables rotate the same item to the top.
#[test]
fn roll_moves_the_item_at_the_given_depth_on_both_tables() {
    let script = ops(&[
        OpCode::PUSH1,
        OpCode::PUSH2,
        OpCode::PUSH3,
        OpCode::PUSH2,
        OpCode::ROLL,
        OpCode::RET,
    ]);

    for table in [JumpTable::default(), JumpTable::not_huyao()] {
        let (state, top) = run(script.clone(), table);
        assert_eq!(state, VmState::HALT);
        assert_eq!(top, Some(StackItem::from_int(1)));
    }
}

/// The pre-606 handler stores into arrays, structs, maps and buffers like the
/// C# `SetItem_Before606` body does. Each script keeps the collection on the
/// stack (`DUP`) because `SETITEM` consumes it, which also selects the
/// stack-referenced branch of the reference bookkeeping.
#[test]
fn setitem_stores_into_arrays_structs_maps_and_buffers_below_huyao() {
    // Array: [null, null] with slot 0 set to 7.
    let array_script = ops(&[
        OpCode::PUSH2,
        OpCode::NEWARRAY,
        OpCode::DUP,
        OpCode::PUSH0,
        OpCode::PUSH7,
        OpCode::SETITEM,
        OpCode::PUSH0,
        OpCode::PICKITEM,
        OpCode::RET,
    ]);
    let (state, top) = run(array_script, JumpTable::not_huyao());
    assert_eq!(state, VmState::HALT);
    assert_eq!(top, Some(StackItem::from_int(7)));

    // Struct: [1] with slot 0 set to 5 (C# covers structs via the VMArray arm).
    let struct_script = ops(&[
        OpCode::NEWSTRUCT0,
        OpCode::DUP,
        OpCode::PUSH1,
        OpCode::APPEND,
        OpCode::DUP,
        OpCode::PUSH0,
        OpCode::PUSH5,
        OpCode::SETITEM,
        OpCode::PUSH0,
        OpCode::PICKITEM,
        OpCode::RET,
    ]);
    let (state, top) = run(struct_script, JumpTable::not_huyao());
    assert_eq!(state, VmState::HALT);
    assert_eq!(top, Some(StackItem::from_int(5)));

    // Map: key 1 set to 5 (a fresh key gains a reference on a referenced map).
    let map_script = ops(&[
        OpCode::NEWMAP,
        OpCode::DUP,
        OpCode::PUSH1,
        OpCode::PUSH5,
        OpCode::SETITEM,
        OpCode::PUSH1,
        OpCode::PICKITEM,
        OpCode::RET,
    ]);
    let (state, top) = run(map_script, JumpTable::not_huyao());
    assert_eq!(state, VmState::HALT);
    assert_eq!(top, Some(StackItem::from_int(5)));

    // Buffer: slot 0 written with 7; PICKITEM on a byte sequence yields the
    // byte payload, which reads back as the integer 7 either way.
    let buffer_script = ops(&[
        OpCode::PUSH5,
        OpCode::NEWBUFFER,
        OpCode::DUP,
        OpCode::PUSH0,
        OpCode::PUSH7,
        OpCode::SETITEM,
        OpCode::PUSH0,
        OpCode::PICKITEM,
        OpCode::RET,
    ]);
    let (state, top) = run(buffer_script, JumpTable::not_huyao());
    assert_eq!(state, VmState::HALT);
    let top = top.expect("buffer slot value");
    assert!(
        top == StackItem::from_int(7) || top == StackItem::ByteString(vec![7]),
        "buffer slot must read back as 7, got {top:?}"
    );
}

/// Storing a collection into itself leaves the target unreferenced once the
/// pops release its stack slots, exercising the branch that adjusts no
/// references at all. The self-reference is uncoupled from the counter there
/// (C# `SetItem_Before606` skips both adjustments when the collection is not
/// stack referenced), so the script still halts cleanly.
#[test]
fn setitem_into_an_unreferenced_collection_adjusts_no_references() {
    // PUSH1 NEWARRAY DUP PUSH0 ROT SETITEM RET: a one-slot array stores
    // itself into its own slot; the popped collection has no stack references
    // left, so the self-reference is stored uncounted.
    let script = ops(&[
        OpCode::PUSH1,
        OpCode::NEWARRAY,
        OpCode::DUP,
        OpCode::PUSH0,
        OpCode::ROT,
        OpCode::SETITEM,
        OpCode::RET,
    ]);

    let (state, top) = run(script.clone(), JumpTable::not_huyao());
    assert_eq!(state, VmState::HALT);
    assert_eq!(top, None);

    // The default table reaches the same net reference state (the popped
    // value's own reference is released instead), so it halts as well.
    let (state, _) = run(script, JumpTable::default());
    assert_eq!(state, VmState::HALT);
}

/// The pre-606 out-of-range throw goes through `ExecuteThrow`, so a script
/// can catch it; without a catch block the engine faults.
#[test]
fn setitem_out_of_range_is_catchable_below_huyao() {
    // TRY { PUSH2 NEWARRAY PUSH5 PUSH7 SETITEM ENDTRY } CATCH { DROP } RET
    let script = vec![
        OpCode::TRY.byte(),
        10, // catch block
        0,  // no finally block
        OpCode::PUSH2.byte(),
        OpCode::NEWARRAY.byte(),
        OpCode::PUSH5.byte(),
        OpCode::PUSH7.byte(),
        OpCode::SETITEM.byte(),
        OpCode::ENDTRY.byte(),
        5,
        OpCode::DROP.byte(), // catch: the exception
        OpCode::ENDTRY.byte(),
        2,
        OpCode::RET.byte(),
    ];

    let (state, top) = run(script, JumpTable::not_huyao());
    assert_eq!(
        state,
        VmState::HALT,
        "the out-of-range throw must be catchable"
    );
    assert_eq!(top, None, "the catch block consumed the exception");

    let script = ops(&[
        OpCode::PUSH2,
        OpCode::NEWARRAY,
        OpCode::PUSH5,
        OpCode::PUSH7,
        OpCode::SETITEM,
        OpCode::RET,
    ]);
    let (state, _) = run(script, JumpTable::not_huyao());
    assert_eq!(
        state,
        VmState::FAULT,
        "uncaught, the throw faults the engine"
    );
}
