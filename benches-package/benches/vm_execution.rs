use criterion::{black_box, criterion_group, criterion_main, Criterion};
use neo_vm::{
    EvaluationStack, ExecutionEngine, OpCode, ReferenceCounter, Script, ScriptBuilder, StackItem,
};

/// Benchmark opcode dispatch: PUSH1+PUSH1+ADD+DROP repeated 1000 times.
fn bench_vm_add_loop(c: &mut Criterion) {
    let mut script_bytes = Vec::new();
    for _ in 0..1000 {
        script_bytes.push(OpCode::PUSH1 as u8); // push 1
        script_bytes.push(OpCode::PUSH1 as u8); // push 1
        script_bytes.push(OpCode::ADD as u8); // add
        script_bytes.push(OpCode::DROP as u8); // drop result
    }
    script_bytes.push(OpCode::RET as u8);

    let script = Script::new(script_bytes, false).expect("valid script");

    c.bench_function("vm_add_loop_1000", |b| {
        b.iter(|| {
            let mut engine = ExecutionEngine::new(None);
            engine
                .load_script(black_box(script.clone()), -1, 0)
                .expect("load script");
            engine.execute();
        });
    });
}

/// Benchmark opcode dispatch: NOP repeated N times (minimal per-instruction overhead).
fn bench_vm_nop_loop(c: &mut Criterion) {
    let mut script_bytes = Vec::with_capacity(5001);
    for _ in 0..5000 {
        script_bytes.push(OpCode::NOP as u8);
    }
    script_bytes.push(OpCode::RET as u8);

    let script = Script::new(script_bytes, false).expect("valid script");

    c.bench_function("vm_nop_5000", |b| {
        b.iter(|| {
            let mut engine = ExecutionEngine::new(None);
            engine
                .load_script(black_box(script.clone()), -1, 0)
                .expect("load script");
            engine.execute();
        });
    });
}

/// Benchmark EvaluationStack push/pop cycles.
fn bench_stack_push_pop(c: &mut Criterion) {
    c.bench_function("stack_push_pop_1000", |b| {
        b.iter(|| {
            let rc = ReferenceCounter::new();
            let mut stack = EvaluationStack::new(rc);
            for i in 0..1000i64 {
                stack.push(StackItem::from_int(black_box(i))).expect("push");
            }
            for _ in 0..1000 {
                let _ = black_box(stack.pop().expect("pop"));
            }
        });
    });
}

/// Benchmark EvaluationStack peek operations.
fn bench_stack_peek(c: &mut Criterion) {
    let rc = ReferenceCounter::new();
    let mut stack = EvaluationStack::new(rc);
    for i in 0..100i64 {
        stack.push(StackItem::from_int(i)).expect("push");
    }

    c.bench_function("stack_peek_top_10000", |b| {
        b.iter(|| {
            for _ in 0..10_000 {
                let _ = black_box(stack.peek(0).expect("peek"));
            }
        });
    });
}

/// Benchmark Script::new() parsing with various sizes.
fn bench_script_parse(c: &mut Criterion) {
    let mut group = c.benchmark_group("script_parse");

    for &size in &[100, 1000, 10_000] {
        // Build a script of the given size: fill with NOP, end with RET
        let mut bytes = vec![OpCode::NOP as u8; size];
        if !bytes.is_empty() {
            *bytes.last_mut().unwrap() = OpCode::RET as u8;
        }

        group.bench_function(format!("{size}_bytes"), |b| {
            b.iter(|| {
                let script = Script::new(black_box(bytes.clone()), false).expect("valid script");
                black_box(script);
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
