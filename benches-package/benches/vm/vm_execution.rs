#![allow(missing_docs)] // benchmark/integration-test harness: not public API
use base64::{Engine as _, engine::general_purpose::STANDARD as BASE64_STANDARD};
use criterion::{Criterion, black_box, criterion_group, criterion_main};
use neo_execution::specialization::try_flamingo_pair_key;
use neo_primitives::constants::MAINNET_MAGIC;
use neo_primitives::{CallFlags, Hardfork, TriggerType, UInt160};
use neo_vm::evaluation_stack::EvaluationStack;
use neo_vm::interop_service::VmInteropDescriptor;
use neo_vm::reference_counter::ReferenceCounter;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm::{
    ContractResolutionIdentity, ExecutionEngine, ExecutionPlan, ExecutionPlanKey,
    ExecutionPlanLimits, HardforkPlanState, HardforkTableIdentity, InteropService, OpCode,
    ProtocolIdentity, ProtocolVersion, Script, Slot, StackItem, VmResult, VmState,
    validate_strict_script,
};
use std::borrow::Cow;
use std::str::FromStr;
use std::sync::Arc;

fn execute(script: &[u8]) -> VmState {
    let mut engine = ExecutionEngine::<()>::new(None);
    engine
        .load_script(Script::new_relaxed(script.to_vec()), -1, 0)
        .expect("load benchmark script");
    engine.execute()
}

fn execute_reused_script(script: &Arc<Script>) -> VmState {
    let mut engine = ExecutionEngine::<()>::new(None);
    let context = engine
        .create_context_from_script_arc(Arc::clone(script), -1, 0)
        .expect("create benchmark context");
    engine
        .load_context(context)
        .expect("load benchmark context");
    engine.execute()
}

fn execute_planned_script(script: &Script, plan: &Arc<ExecutionPlan>) -> VmState {
    let mut engine = ExecutionEngine::<()>::new(None);
    engine
        .load_script_with_plan(script.clone(), Arc::clone(plan), -1, 0)
        .expect("load planned benchmark script");
    engine.execute()
}

fn build_plan(script: &[u8]) -> ExecutionPlan {
    build_plan_with_identity(
        script,
        0,
        0x334f_454e,
        HardforkTableIdentity::unconfigured(),
        None,
    )
}

fn build_plan_with_identity(
    script: &[u8],
    entry: u32,
    network_magic: u32,
    hardforks: HardforkTableIdentity,
    contract: Option<ContractResolutionIdentity>,
) -> ExecutionPlan {
    let key = ExecutionPlanKey::new(
        Arc::<[u8]>::from(script),
        entry,
        ProtocolIdentity::new(network_magic, ProtocolVersion::NEO_N3_V3_10_1),
        hardforks,
        TriggerType::APPLICATION,
        contract,
    );
    ExecutionPlan::build(key, ExecutionPlanLimits::default()).expect("build benchmark plan")
}

fn execute_template(script: &Script, plan: Option<&Arc<ExecutionPlan>>) -> VmState {
    match plan {
        Some(plan) => execute_planned_script(script, plan),
        None => {
            let mut engine = ExecutionEngine::<()>::new(None);
            engine
                .load_script(script.clone(), -1, 0)
                .expect("load template script");
            engine.execute()
        }
    }
}

fn no_op_syscall(_engine: &mut ExecutionEngine<()>) -> VmResult<()> {
    Ok(())
}

fn configure_no_op_syscall(engine: &mut ExecutionEngine<()>) -> u32 {
    let mut service = InteropService::with_capacity(1);
    let hash = service
        .register(VmInteropDescriptor {
            name: Cow::Borrowed("System.Benchmark.NoOp"),
            handler: Some(no_op_syscall),
            price: 0,
            required_call_flags: CallFlags::NONE,
        })
        .expect("register benchmark syscall");
    engine.set_interop_service(service);
    hash
}

fn execute_syscall_template(script: &Script, plan: Option<&Arc<ExecutionPlan>>) -> VmState {
    let mut engine = ExecutionEngine::<()>::new(None);
    configure_no_op_syscall(&mut engine);
    match plan {
        Some(plan) => engine
            .load_script_with_plan(script.clone(), Arc::clone(plan), -1, 0)
            .expect("load planned syscall script"),
        None => engine
            .load_script(script.clone(), -1, 0)
            .expect("load ordinary syscall script"),
    };
    engine.execute()
}

fn mainnet_factory_script() -> Vec<u8> {
    const CONTRACT_MAP: &str = include_str!(
        "../../../reports/performance/mainnet-vm-script-contract-map-1887000-1897000.json"
    );
    const SCRIPT_HASH: &str = "0x0993009a4e794f2e2071fb9489eef5aff390ea97";
    let report: serde_json::Value = serde_json::from_str(CONTRACT_MAP).expect("contract map JSON");
    let encoded = report["matches"]
        .as_array()
        .expect("contract matches")
        .iter()
        .find(|record| record["raw_script_hash"].as_str() == Some(SCRIPT_HASH))
        .and_then(|record| record["script_base64"].as_str())
        .expect("FlamingoSwapFactory script");
    BASE64_STANDARD
        .decode(encoded)
        .expect("decode MainNet contract script")
}

fn mainnet_factory_hardforks() -> HardforkTableIdentity {
    HardforkTableIdentity::unconfigured()
        .with_state(
            Hardfork::HfAspidochelone,
            HardforkPlanState::Active {
                activation_height: 1_730_000,
            },
        )
        .with_state(
            Hardfork::HfBasilisk,
            HardforkPlanState::Pending {
                activation_height: 4_120_000,
            },
        )
        .with_state(
            Hardfork::HfCockatrice,
            HardforkPlanState::Pending {
                activation_height: 5_450_000,
            },
        )
        .with_state(
            Hardfork::HfDomovoi,
            HardforkPlanState::Pending {
                activation_height: 5_570_000,
            },
        )
        .with_state(
            Hardfork::HfEchidna,
            HardforkPlanState::Pending {
                activation_height: 7_300_000,
            },
        )
        .with_state(
            Hardfork::HfFaun,
            HardforkPlanState::Pending {
                activation_height: 8_800_000,
            },
        )
        .with_state(
            Hardfork::HfGorgon,
            HardforkPlanState::Pending {
                activation_height: 12_020_000,
            },
        )
}

fn execute_mainnet_pair_key(script: &Script, plan: Option<&Arc<ExecutionPlan>>) -> VmState {
    const ENTRY: usize = 391;
    let mut engine = ExecutionEngine::<()>::new(None);
    match plan {
        Some(plan) => engine
            .load_script_with_plan(script.clone(), Arc::clone(plan), -1, ENTRY)
            .expect("load planned MainNet helper"),
        None => engine
            .load_script(script.clone(), -1, ENTRY)
            .expect("load ordinary MainNet helper"),
    };

    let reference_counter = engine.reference_counter().clone();
    let context = engine
        .current_context_mut()
        .expect("MainNet helper context");
    context.set_static_fields(Some(Slot::new(2, reference_counter)));
    context
        .push(StackItem::from_buffer(vec![0xff]))
        .expect("push static prefix");
    let prefix = context.pop().expect("pop static prefix");
    context
        .store_static_field(1, prefix)
        .expect("set static prefix");
    context
        .push(StackItem::from_byte_string(vec![0x22; 20]))
        .expect("push second argument");
    context
        .push(StackItem::from_byte_string(vec![0x11; 20]))
        .expect("push first argument");
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

/// Benchmark steady-state dispatch after a relaxed script's instruction cache is warm.
fn bench_vm_nop_loop_reused_script(c: &mut Criterion) {
    let mut script_bytes = vec![OpCode::NOP.byte(); 5000];
    script_bytes.push(OpCode::RET.byte());
    let script = Arc::new(Script::new_relaxed(script_bytes));

    c.bench_function("vm_nop_5000_reused_script", |b| {
        b.iter(|| {
            assert_eq!(execute_reused_script(black_box(&script)), VmState::Halt);
        });
    });
}

/// Benchmark opt-in straight-line execution through immutable plan references.
fn bench_vm_nop_loop_planned(c: &mut Criterion) {
    let mut script_bytes = vec![OpCode::NOP.byte(); 5000];
    script_bytes.push(OpCode::RET.byte());
    let script = Script::new_relaxed(script_bytes.clone());
    let plan = Arc::new(build_plan(&script_bytes));

    c.bench_function("vm_nop_5000_planned", |b| {
        b.iter(|| {
            assert_eq!(
                execute_planned_script(black_box(&script), black_box(&plan)),
                VmState::Halt
            );
        });
    });
}

/// Compare the current shared lazy instruction cache with a plan's direct
/// byte-offset index. Both inputs are fully warm before Criterion samples.
fn bench_warm_instruction_lookup(c: &mut Criterion) {
    let mut script_bytes = vec![OpCode::NOP.byte(); 5000];
    script_bytes.push(OpCode::RET.byte());
    let script = Script::new_relaxed(script_bytes.clone());
    for offset in 0..script.len() {
        black_box(script.get_instruction(offset).expect("warm instruction"));
    }
    let plan = build_plan(&script_bytes);

    let mut group = c.benchmark_group("vm_warm_instruction_lookup_5001");
    group.bench_function("script_atomic_arc", |b| {
        b.iter(|| {
            for offset in 0..script.len() {
                black_box(script.get_instruction(offset).expect("cached instruction"));
            }
        });
    });
    group.bench_function("plan_direct_index", |b| {
        b.iter(|| {
            for offset in 0..script.len() {
                black_box(plan.instruction_at(offset).expect("planned instruction"));
            }
        });
    });
    group.finish();
}

/// Benchmark cold script construction against strict immutable plan building.
fn bench_cold_plan_construction(c: &mut Criterion) {
    let mut script_bytes = vec![OpCode::NOP.byte(); 5000];
    script_bytes.push(OpCode::RET.byte());
    let mut group = c.benchmark_group("vm_cold_plan_5001");
    group.bench_function("relaxed_script", |b| {
        b.iter(|| black_box(Script::new_relaxed(black_box(script_bytes.clone()))));
    });
    group.bench_function("strict_plan", |b| {
        b.iter(|| black_box(build_plan(black_box(&script_bytes))));
    });
    group.finish();
}

/// Branch-heavy code forces a planned block boundary every two instructions.
fn bench_branch_heavy_plan(c: &mut Criterion) {
    let mut bytes = Vec::with_capacity(3001);
    for _ in 0..1000 {
        bytes.extend_from_slice(&[OpCode::PUSH1.byte(), OpCode::JMPIF.byte(), 2]);
    }
    bytes.push(OpCode::RET.byte());
    let script = Script::new_relaxed(bytes.clone());
    let plan = Arc::new(build_plan(&bytes));
    assert_eq!(execute_template(&script, None), VmState::Halt);
    assert_eq!(execute_template(&script, Some(&plan)), VmState::Halt);

    let mut group = c.benchmark_group("vm_branch_heavy_1000");
    group.bench_function("ordinary", |b| {
        b.iter(|| black_box(execute_template(&script, None)));
    });
    group.bench_function("planned", |b| {
        b.iter(|| black_box(execute_template(&script, Some(&plan))));
    });
    group.finish();
}

/// Syscall-heavy code measures the conservative one-block-per-host-call path.
fn bench_syscall_heavy_plan(c: &mut Criterion) {
    let mut setup_engine = ExecutionEngine::<()>::new(None);
    let syscall = configure_no_op_syscall(&mut setup_engine);
    let mut bytes = Vec::with_capacity(5001);
    for _ in 0..1000 {
        bytes.push(OpCode::SYSCALL.byte());
        bytes.extend_from_slice(&syscall.to_le_bytes());
    }
    bytes.push(OpCode::RET.byte());
    let script = Script::new_relaxed(bytes.clone());
    let plan = Arc::new(build_plan(&bytes));
    assert_eq!(execute_syscall_template(&script, None), VmState::Halt);
    assert_eq!(
        execute_syscall_template(&script, Some(&plan)),
        VmState::Halt
    );

    let mut group = c.benchmark_group("vm_syscall_heavy_1000");
    group.bench_function("ordinary", |b| {
        b.iter(|| black_box(execute_syscall_template(&script, None)));
    });
    group.bench_function("planned", |b| {
        b.iter(|| black_box(execute_syscall_template(&script, Some(&plan))));
    });
    group.finish();
}

/// Execute the trace-selected MainNet FlamingoSwapFactory pair-key helper.
fn bench_mainnet_pair_key_plan(c: &mut Criterion) {
    const ENTRY: u32 = 391;
    let bytes = mainnet_factory_script();
    let script = Script::new_relaxed(bytes.clone());
    let contract_hash = UInt160::from_str("0xca2d20610d7982ebe0bed124ee7e9b2d580a6efc")
        .expect("MainNet contract hash");
    let plan = Arc::new(build_plan_with_identity(
        &bytes,
        ENTRY,
        MAINNET_MAGIC,
        mainnet_factory_hardforks(),
        Some(ContractResolutionIdentity::new(
            contract_hash,
            27,
            1,
            2_962_741_568,
        )),
    ));
    assert_eq!(execute_mainnet_pair_key(&script, None), VmState::Halt);
    assert_eq!(
        execute_mainnet_pair_key(&script, Some(&plan)),
        VmState::Halt
    );

    let mut group = c.benchmark_group("vm_mainnet_flamingo_pair_key");
    group.bench_function("ordinary", |b| {
        b.iter(|| black_box(execute_mainnet_pair_key(&script, None)));
    });
    group.bench_function("planned", |b| {
        b.iter(|| black_box(execute_mainnet_pair_key(&script, Some(&plan))));
    });
    let arguments = [
        StackItem::from_byte_string(vec![0x11; 20]),
        StackItem::from_byte_string(vec![0x22; 20]),
    ];
    let prefix = StackItem::from_buffer(vec![0xFF]);
    group.bench_function("specialized_kernel", |b| {
        b.iter(|| {
            black_box(
                try_flamingo_pair_key(
                    black_box(&arguments),
                    black_box(&prefix),
                    black_box(false),
                    black_box(false),
                )
                .expect("exact candidate invocation"),
            );
        });
    });
    group.finish();
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
    bench_vm_nop_loop_reused_script,
    bench_vm_nop_loop_planned,
    bench_warm_instruction_lookup,
    bench_cold_plan_construction,
    bench_branch_heavy_plan,
    bench_syscall_heavy_plan,
    bench_mainnet_pair_key_plan,
    bench_stack_push_pop,
    bench_stack_peek,
    bench_script_parse,
    bench_script_builder,
);
criterion_main!(benches);
