use neo_vm::execution_engine::{ExecutionEngine, VMState};
use neo_vm::op_code::OpCode;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm::stack_item::StackItem;

#[test]
fn host_pushed_untracked_compound_item_gets_reference_counter() {
    let mut engine = ExecutionEngine::new(None);

    let script = ScriptBuilder::new()
        .emit_opcode(OpCode::PUSH1)
        .emit_opcode(OpCode::PACK)
        .emit_opcode(OpCode::RET)
        .to_script();

    engine.load_script(script, -1, 0).expect("load_script");

    // The host creates compound items without a reference counter. The VM must
    // attach its own counter when the item is pushed, otherwise compound opcodes
    // like PACK could fault (or previously panic).
    engine
        .push(StackItem::from_array(vec![StackItem::from_int(1)]))
        .expect("push host array");

    let state = engine.execute();
    assert_eq!(state, VMState::HALT);

    let stack = engine.result_stack();
    assert_eq!(stack.len(), 1);

    let packed_items = stack.peek(0).unwrap().as_array().unwrap();
    assert_eq!(packed_items.len(), 1);

    let inner_items = packed_items[0].as_array().unwrap();
    assert_eq!(inner_items.len(), 1);
    assert_eq!(inner_items[0].as_int().unwrap(), 1.into());
}
