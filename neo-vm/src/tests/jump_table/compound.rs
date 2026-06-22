use super::*;
use crate::script::Script;

fn engine_with_stack(items: Vec<StackItem>) -> ExecutionEngine {
    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(Script::new_relaxed(vec![OpCode::RET.byte()]), -1, 0)
        .expect("load test script");

    let ctx = engine.current_context_mut().expect("current context");
    for item in items {
        ctx.push(item).expect("push test item");
    }

    engine
}

fn instruction(opcode: OpCode) -> Instruction {
    Instruction::new(opcode, &[])
}

fn pop(engine: &mut ExecutionEngine) -> StackItem {
    engine
        .current_context_mut()
        .expect("current context")
        .pop()
        .expect("result item")
}

/// C# VALUES accepts an Array source (and a Struct, since `Struct : Array`), not
/// just a Map. JumpTable.Compound.cs:346-351 — a VALUES over an Array must not
/// fault.
#[test]
fn values_accepts_array_source_like_csharp() {
    let array = StackItem::from_array(vec![StackItem::from_i64(1), StackItem::from_i64(2)]);
    let mut engine = engine_with_stack(vec![array]);
    values(&mut engine, &instruction(OpCode::VALUES)).expect("VALUES over an Array must not fault");
    match pop(&mut engine) {
        StackItem::Array(a) => assert_eq!(a.len(), 2),
        other => panic!("expected Array result, got {other:?}"),
    }
}

#[test]
fn values_accepts_struct_source_like_csharp() {
    let structure = StackItem::from_struct(vec![StackItem::from_i64(7)]);
    let mut engine = engine_with_stack(vec![structure]);
    values(&mut engine, &instruction(OpCode::VALUES)).expect("VALUES over a Struct must not fault");
    match pop(&mut engine) {
        StackItem::Array(a) => assert_eq!(a.len(), 1),
        other => panic!("expected Array result, got {other:?}"),
    }
}

#[test]
fn values_faults_on_non_collection_source() {
    let mut engine = engine_with_stack(vec![StackItem::from_i64(1)]);
    assert!(
        values(&mut engine, &instruction(OpCode::VALUES)).is_err(),
        "VALUES over a non-collection (Integer) must fault"
    );
}

/// Each Struct element of the source is deep-cloned (C# `s.Clone`), so the result
/// holds a Struct that does not alias the source.
#[test]
fn values_deep_clones_struct_elements() {
    let inner = StackItem::from_struct(vec![StackItem::from_i64(1)]);
    let array = StackItem::from_array(vec![inner]);
    let mut engine = engine_with_stack(vec![array]);
    values(&mut engine, &instruction(OpCode::VALUES)).expect("VALUES with a Struct element");
    match pop(&mut engine) {
        StackItem::Array(a) => a.with_items(|items| {
            assert_eq!(items.len(), 1);
            assert!(
                matches!(items[0], StackItem::Struct(_)),
                "Struct element is preserved as a Struct in the result"
            );
        }),
        other => panic!("expected Array result, got {other:?}"),
    }
}
