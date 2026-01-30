//! VM Execution Benchmarks
//!
//! Benchmarks for Neo Virtual Machine instruction execution, stack operations,
//! and script execution with realistic workloads.

use criterion::{black_box, criterion_group, criterion_main, BenchmarkId, Criterion, Throughput};
use neo_vm::{op_code::OpCode, ExecutionEngine, Script, ScriptBuilder, VMState};

/// Creates a script that performs arithmetic operations
fn create_arithmetic_script(iterations: usize) -> Script {
    let mut builder = ScriptBuilder::new();

    // Push initial value
    builder.emit_push_int(1);

    // Loop: duplicate and add
    for _ in 0..iterations {
        builder.emit_opcode(OpCode::DUP);
        builder.emit_push_int(1);
        builder.emit_opcode(OpCode::ADD);
    }

    // Return result
    builder.emit_opcode(OpCode::RET);

    Script::new(builder.to_array(), false).unwrap()
}

/// Creates a script that pushes various data sizes
fn create_pushdata_script(size: usize) -> Script {
    let mut builder = ScriptBuilder::new();

    // Push data of specified size
    let data = vec![0xABu8; size];
    builder.emit_push(&data);
    builder.emit_opcode(OpCode::RET);

    Script::new(builder.to_array(), false).unwrap()
}

/// Creates a script with stack manipulation operations
fn create_stack_ops_script(depth: usize) -> Script {
    let mut builder = ScriptBuilder::new();

    // Push values onto stack
    for i in 0..depth {
        builder.emit_push_int(i as i64);
    }

    // Perform various stack operations
    for _ in 0..depth / 2 {
        builder.emit_opcode(OpCode::DUP);
        builder.emit_opcode(OpCode::SWAP);
        builder.emit_opcode(OpCode::DROP);
    }

    // Clean up remaining stack
    for _ in 0..depth / 2 + 1 {
        builder.emit_opcode(OpCode::DROP);
    }

    builder.emit_opcode(OpCode::RET);

    Script::new(builder.to_array(), false).unwrap()
}

/// Creates a script with bitwise operations
fn create_bitwise_script(iterations: usize) -> Script {
    let mut builder = ScriptBuilder::new();

    // Push operands
    builder.emit_push_int(0xFF00FF00i64);
    builder.emit_push_int(0x0F0F0F0Fi64);

    // Perform bitwise operations
    for _ in 0..iterations {
        builder.emit_opcode(OpCode::DUP);
        builder.emit_opcode(OpCode::OVER);
        builder.emit_opcode(OpCode::AND);
        builder.emit_opcode(OpCode::DROP);
        builder.emit_opcode(OpCode::DUP);
        builder.emit_opcode(OpCode::OVER);
        builder.emit_opcode(OpCode::OR);
        builder.emit_opcode(OpCode::DROP);
        builder.emit_opcode(OpCode::DUP);
        builder.emit_opcode(OpCode::OVER);
        builder.emit_opcode(OpCode::XOR);
        builder.emit_opcode(OpCode::DROP);
    }

    // Clean up
    builder.emit_opcode(OpCode::DROP);
    builder.emit_opcode(OpCode::DROP);
    builder.emit_opcode(OpCode::RET);

    Script::new(builder.to_array(), false).unwrap()
}

fn bench_arithmetic_ops(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_arithmetic");

    for iterations in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*iterations as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(iterations),
            iterations,
            |b, &iterations| {
                let script = create_arithmetic_script(iterations);
                b.iter(|| {
                    let mut engine = ExecutionEngine::new(None);
                    engine
                        .load_script(black_box(script.clone()), -1, 0)
                        .unwrap();
                    let state = engine.execute();
                    assert_eq!(state, VMState::HALT);
                });
            },
        );
    }

    group.finish();
}

fn bench_pushdata_ops(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_pushdata");

    for size in [32, 256, 1024, 4096].iter() {
        group.throughput(Throughput::Bytes(*size as u64));
        group.bench_with_input(BenchmarkId::from_parameter(size), size, |b, &size| {
            let script = create_pushdata_script(size);
            b.iter(|| {
                let mut engine = ExecutionEngine::new(None);
                engine
                    .load_script(black_box(script.clone()), -1, 0)
                    .unwrap();
                let state = engine.execute();
                assert_eq!(state, VMState::HALT);
            });
        });
    }

    group.finish();
}

fn bench_stack_ops(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_stack_ops");

    for depth in [10, 50, 100].iter() {
        group.throughput(Throughput::Elements(*depth as u64));
        group.bench_with_input(BenchmarkId::from_parameter(depth), depth, |b, &depth| {
            let script = create_stack_ops_script(depth);
            b.iter(|| {
                let mut engine = ExecutionEngine::new(None);
                engine
                    .load_script(black_box(script.clone()), -1, 0)
                    .unwrap();
                let state = engine.execute();
                assert_eq!(state, VMState::HALT);
            });
        });
    }

    group.finish();
}

fn bench_bitwise_ops(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_bitwise");

    for iterations in [10, 100, 1000].iter() {
        group.throughput(Throughput::Elements(*iterations as u64));
        group.bench_with_input(
            BenchmarkId::from_parameter(iterations),
            iterations,
            |b, &iterations| {
                let script = create_bitwise_script(iterations);
                b.iter(|| {
                    let mut engine = ExecutionEngine::new(None);
                    engine
                        .load_script(black_box(script.clone()), -1, 0)
                        .unwrap();
                    let state = engine.execute();
                    assert_eq!(state, VMState::HALT);
                });
            },
        );
    }

    group.finish();
}

fn bench_individual_opcodes(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_opcodes");

    // Benchmark individual opcode execution
    group.bench_function("opcode_nop", |b| {
        let script = Script::new(vec![OpCode::NOP as u8, OpCode::RET as u8], false).unwrap();
        b.iter(|| {
            let mut engine = ExecutionEngine::new(None);
            engine
                .load_script(black_box(script.clone()), -1, 0)
                .unwrap();
            engine.execute()
        });
    });

    group.bench_function("opcode_push1_add", |b| {
        let script = Script::new(
            vec![
                OpCode::PUSH1 as u8,
                OpCode::PUSH2 as u8,
                OpCode::ADD as u8,
                OpCode::RET as u8,
            ],
            false,
        )
        .unwrap();
        b.iter(|| {
            let mut engine = ExecutionEngine::new(None);
            engine
                .load_script(black_box(script.clone()), -1, 0)
                .unwrap();
            engine.execute()
        });
    });

    group.bench_function("opcode_dup_drop", |b| {
        let mut builder = ScriptBuilder::new();
        builder.emit_push_int(42);
        for _ in 0..100 {
            builder.emit_opcode(OpCode::DUP);
            builder.emit_opcode(OpCode::DROP);
        }
        builder.emit_opcode(OpCode::RET);
        let script = Script::new(builder.to_array(), false).unwrap();

        b.iter(|| {
            let mut engine = ExecutionEngine::new(None);
            engine
                .load_script(black_box(script.clone()), -1, 0)
                .unwrap();
            engine.execute()
        });
    });

    group.finish();
}

fn bench_script_creation(c: &mut Criterion) {
    let mut group = c.benchmark_group("vm_script_creation");

    group.bench_function("script_builder_simple", |b| {
        b.iter(|| {
            let mut builder = ScriptBuilder::new();
            builder.emit_push_int(42);
            builder.emit_opcode(OpCode::DUP);
            builder.emit_opcode(OpCode::ADD);
            builder.emit_opcode(OpCode::RET);
            builder.to_array()
        });
    });

    group.bench_function("script_builder_complex", |b| {
        b.iter(|| {
            let mut builder = ScriptBuilder::new();
            for i in 0..100 {
                builder.emit_push_int(i);
            }
            builder.emit_push_int(100);
            builder.emit_opcode(OpCode::PACK);
            builder.emit_opcode(OpCode::RET);
            builder.to_array()
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_arithmetic_ops,
    bench_pushdata_ops,
    bench_stack_ops,
    bench_bitwise_ops,
    bench_individual_opcodes,
    bench_script_creation
);
criterion_main!(benches);
