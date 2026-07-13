#![allow(missing_docs)] // benchmark/integration-test harness: not public API
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use neo_vm::evaluation_stack::EvaluationStack;
use neo_vm::reference_counter::ReferenceCounter;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm::{ExecutionEngine, OpCode, Script, StackItem, VmState, validate_strict_script};

fn execute(script: &[u8]) -> VmState {
    let mut engine = ExecutionEngine::<()>::new(None);
    engine
        .load_script(Script::new_relaxed(script.to_vec()), -1, 0)
        .expect("load benchmark script");
    engine.execute()
}

/// Benchmark opcode dispatch: PUSH1+PUSH1+ADD+DROP repeated 1000 times.
fn bench_vm_add_loop(c: &mut Criterion) {
    let mut script_bytes = Vec::new();
    for _ in 0..1000 {
        script_bytes.push(OpCode::PUSH1.byte()); // push 1
        script_bytes.push(OpCode::PUSH1.byte()); // push 1
        script_bytes.push(OpCode::ADD.byte()); // add
        script_bytes.push(OpCode::DROP.byte()); // drop result
    }
    script_bytes.push(OpCode::RET.byte());

    validate_strict_script(&script_bytes).expect("valid script");

    c.bench_function("vm_add_loop_1000", |b| {
        b.iter(|| {
            assert_eq!(execute(black_box(&script_bytes)), VmState::Halt);
        });
    });
}

/// Benchmark opcode dispatch: NOP repeated N times (minimal per-instruction overhead).
fn bench_vm_nop_loop(c: &mut Criterion) {
    let mut script_bytes = Vec::with_capacity(5001);
    for _ in 0..5000 {
        script_bytes.push(OpCode::NOP.byte());
    }
    script_bytes.push(OpCode::RET.byte());

    validate_strict_script(&script_bytes).expect("valid script");

    c.bench_function("vm_nop_5000", |b| {
        b.iter(|| {
            assert_eq!(execute(black_box(&script_bytes)), VmState::Halt);
        });
    });
}

/// Benchmark local VM evaluation-stack push/pop cycles.
fn bench_stack_push_pop(c: &mut Criterion) {
    c.bench_function("stack_push_pop_1000", |b| {
        b.iter(|| {
            let mut stack = EvaluationStack::new(ReferenceCounter::new());
            for i in 0..1000i64 {
                stack
                    .push(StackItem::from_i64(black_box(i)))
                    .expect("push benchmark item");
            }
            for _ in 0..1000 {
                let _ = black_box(stack.pop().expect("pop benchmark item"));
            }
        });
    });
}

/// Benchmark stack peek operations.
fn bench_stack_peek(c: &mut Criterion) {
    let mut stack = EvaluationStack::new(ReferenceCounter::new());
    for i in 0..100i64 {
        stack
            .push(StackItem::from_i64(i))
            .expect("push benchmark item");
    }

    c.bench_function("stack_peek_top_10000", |b| {
        b.iter(|| {
            for _ in 0..10_000 {
                let _ = black_box(stack.peek(0).expect("peek benchmark item"));
            }
        });
    });
}

/// Benchmark bytecode validation with various sizes.
fn bench_script_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("script_parse");

    for &size in &[100, 1000, 10_000] {
        // Build a script of the given size: fill with NOP, end with RET
        let mut bytes = vec![OpCode::NOP.byte(); size];
        if !bytes.is_empty() {
            *bytes.last_mut().unwrap() = OpCode::RET.byte();
        }

        group.bench_function(format!("{size}_bytes"), |b| {
            b.iter(|| {
                validate_strict_script(black_box(&bytes)).expect("valid script");
            });
        });
    }
    group.finish();
}

/// Benchmark ScriptBuilder emit throughput.
fn bench_script_builder(c: &mut Criterion) {
    c.bench_function("script_builder_1000_opcodes", |b| {
        b.iter(|| {
            let mut builder = ScriptBuilder::new();
            for _ in 0..1000 {
                builder.emit_opcode(OpCode::PUSH1);
                builder.emit_opcode(OpCode::DROP);
            }
            black_box(builder.to_array());
        });
    });
}

criterion_group!(
    benches,
    bench_vm_add_loop,
    bench_vm_nop_loop,
    bench_stack_push_pop,
    bench_stack_peek,
    bench_script_parse,
    bench_script_builder,
);
criterion_main!(benches);
