use super::*;
use crate::specialization::{
    CandidateRouteConfig, SpecializationControlConfig, SpecializationControlLimits,
};
use neo_manifest::{ContractManifest, NefFile};
use neo_primitives::constants::MAINNET_MAGIC;
use neo_vm::{
    HardforkPlanState, HardforkTableIdentity, OpCode, Slot, SpecializationMode, StackItemType,
    VmExecutionProfile,
};
use std::str::FromStr;

const PROFILE_HEIGHT: u32 = 1_887_001;
const CONTRACT_ID: i32 = 27;
const CONTRACT_UPDATE: u16 = 1;
const CONTRACT_NEF_CHECKSUM: u32 = 2_962_741_568;

#[derive(Clone)]
struct PreparedCase {
    settings: ProtocolSettings,
    profile_height: u32,
    gas_limit: i64,
    wrong_script: bool,
    contract_update: u16,
    shared_frame: bool,
    static_prefix: Option<StackItem>,
    token_a: StackItem,
    token_b: StackItem,
    fee_whitelisted: bool,
    profiling: bool,
    filler_items: usize,
    outer_fault: bool,
}

impl Default for PreparedCase {
    fn default() -> Self {
        Self {
            settings: ProtocolSettings::default(),
            profile_height: PROFILE_HEIGHT,
            gas_limit: TEST_MODE_GAS,
            wrong_script: false,
            contract_update: CONTRACT_UPDATE,
            shared_frame: true,
            static_prefix: Some(StackItem::from_buffer(vec![0xFF])),
            token_a: StackItem::from_byte_string(vec![0x11; 20]),
            token_b: StackItem::from_byte_string(vec![0x22; 20]),
            fee_whitelisted: false,
            profiling: false,
            filler_items: 0,
            outer_fault: false,
        }
    }
}

fn exact_candidate() -> CandidateContract {
    flamingo_pair_key_candidate(HardforkTableIdentity::unconfigured())
        .expect("embedded candidate is valid")
}

fn exact_contract(script: Vec<u8>, update: u16) -> Arc<ContractState> {
    let mut nef = NefFile::new("prepared-frame-test".to_string(), script);
    nef.checksum = CONTRACT_NEF_CHECKSUM;
    let hash = UInt160::from_str("0xca2d20610d7982ebe0bed124ee7e9b2d580a6efc")
        .expect("known contract hash");
    let mut contract = ContractState::new(
        CONTRACT_ID,
        hash,
        nef,
        ContractManifest::new("FlamingoSwapFactory".to_string()),
    );
    contract.update_counter = update;
    Arc::new(contract)
}

fn shadow_control() -> SpecializationControl {
    let candidate = exact_candidate();
    let config = SpecializationControlConfig::try_enabled(
        false,
        SpecializationControlLimits::DEFAULT,
        [CandidateRouteConfig::new(
            FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
            candidate.identity().candidate_version(),
            SpecializationMode::Shadow,
        )],
    )
    .expect("exact shadow control config");
    SpecializationControl::new(config)
}

fn prepared_engine(case: PreparedCase) -> ApplicationEngine<NoNativeContractProvider> {
    let mut block = Block::new();
    block.header.set_index(case.profile_height);
    let mut engine =
        ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            Some(block),
            case.settings,
            case.gas_limit,
            NoDiagnostic,
            Arc::new(NoNativeContractProvider),
        )
        .expect("prepared application engine");

    if case.outer_fault {
        engine
            .load_script(
                vec![
                    OpCode::DROP.byte(),
                    OpCode::DROP.byte(),
                    OpCode::PUSHF.byte(),
                    OpCode::ASSERT.byte(),
                ],
                CallFlags::ALL,
                None,
            )
            .expect("outer faulting caller loads");
    }

    let mut script_bytes = exact_candidate()
        .identity()
        .execution()
        .script_bytes()
        .to_vec();
    if case.wrong_script {
        script_bytes[0] ^= 1;
    }
    let contract = exact_contract(script_bytes.clone(), case.contract_update);
    let contract_hash = contract.hash;
    let script = Script::new_relaxed(script_bytes);
    let caller_position = script.len();
    let state_contract = Arc::clone(&contract);
    let mut caller = engine
        .load_script_with_state(script, -1, caller_position, move |state| {
            state.contract = Some(state_contract);
            state.script_hash = Some(contract_hash);
            state.whitelisted = case.fee_whitelisted;
        })
        .expect("caller context loads");

    if let Some(prefix) = case.static_prefix {
        let reference_counter = caller.reference_counter().clone();
        caller.set_static_fields(Some(Slot::new(2, reference_counter)));
        caller
            .store_static_field(1, prefix)
            .expect("static prefix initializes");
    }
    for _ in 0..case.filler_items {
        caller
            .push(StackItem::from_i64(7))
            .expect("filler item pushes");
    }
    caller
        .push(StackItem::from_i64(0x1234))
        .expect("sentinel pushes");
    caller.push(case.token_b).expect("token B pushes");
    caller.push(case.token_a).expect("token A pushes");

    let callee = if case.shared_frame {
        caller
            .clone_with_position(FLAMINGO_FACTORY_PAIR_KEY_ENTRY as usize)
            .expect("internal CALL clone")
    } else {
        engine
            .vm_engine
            .engine()
            .create_context_from_script_arc(
                caller.script_arc(),
                0,
                FLAMINGO_FACTORY_PAIR_KEY_ENTRY as usize,
            )
            .expect("independent non-CALL frame")
    };
    let attached_here = engine.attach_host();
    let load_result = engine.vm_engine.engine_mut().load_context(callee);
    engine.detach_host(attached_here);
    load_result.expect("callee context loads");
    if case.profiling {
        engine.enable_vm_execution_profiling();
    }
    engine
}

fn current_stack(engine: &ApplicationEngine<NoNativeContractProvider>) -> Vec<StackItem> {
    let context = engine
        .vm_engine
        .engine()
        .current_context()
        .expect("current context");
    let stack = context.evaluation_stack();
    (0..stack.len())
        .map(|index| stack.peek(index).expect("stack item").clone())
        .collect()
}

fn result_stack(engine: &ApplicationEngine<NoNativeContractProvider>) -> Vec<StackItem> {
    let stack = engine.result_stack();
    (0..stack.len())
        .map(|index| stack.peek(index).expect("result item").clone())
        .collect()
}

fn assert_stack_values_equal(left: &[StackItem], right: &[StackItem]) {
    assert_eq!(left.len(), right.len());
    for (left, right) in left.iter().zip(right) {
        assert_eq!(left.stack_item_type(), right.stack_item_type());
        assert_eq!(
            left.as_bytes().expect("simple stack value"),
            right.as_bytes().expect("simple stack value")
        );
    }
}

#[derive(Debug, Eq, PartialEq)]
struct FrameSnapshot {
    state: VMState,
    instructions: u64,
    fee_pico: i64,
    gas_pico: i64,
    references: usize,
    invocation_depth: usize,
    current_ip: Option<usize>,
    evaluation_len: Option<usize>,
    static_len: Option<usize>,
    result_len: usize,
    uncaught_exception: Option<StackItem>,
    fault: Option<String>,
    notifications: usize,
    logs: usize,
    profile: Option<VmExecutionProfile>,
    is_jumping: bool,
}

fn frame_snapshot(engine: &ApplicationEngine<NoNativeContractProvider>) -> FrameSnapshot {
    let context = engine.vm_engine.engine().current_context();
    FrameSnapshot {
        state: engine.state(),
        instructions: engine.instructions_executed(),
        fee_pico: engine.fee_consumed_pico(),
        gas_pico: engine.gas_consumed_pico(),
        references: engine.reference_count(),
        invocation_depth: engine.invocation_stack().len(),
        current_ip: context.map(neo_vm::ExecutionContext::instruction_pointer),
        evaluation_len: context.map(|context| context.evaluation_stack().len()),
        static_len: context.map(neo_vm::ExecutionContext::static_fields_len),
        result_len: engine.result_stack().len(),
        uncaught_exception: engine.uncaught_exception_item().cloned(),
        fault: engine.fault_exception().map(str::to_string),
        notifications: engine.notifications().len(),
        logs: engine.logs().len(),
        profile: engine.vm_execution_profile(),
        is_jumping: engine.vm_engine.engine().is_jumping,
    }
}

fn try_prepared_frame(engine: &mut ApplicationEngine<NoNativeContractProvider>) -> bool {
    let candidate = flamingo_pair_key_candidate(engine.hardfork_plan_identity())
        .expect("height-specific candidate");
    let policy = flamingo_cpu_fee_policy(&candidate).expect("fixed decision fee policy");
    let attached_here = engine.attach_host();
    let result = engine.try_apply_flamingo_pair_key_frame(&candidate, &policy);
    engine.detach_host(attached_here);
    result.expect("eligibility does not fault")
}

fn assert_rejected_without_mutation(
    label: &str,
    mut engine: ApplicationEngine<NoNativeContractProvider>,
) {
    let before = frame_snapshot(&engine);
    assert!(!try_prepared_frame(&mut engine), "{label} must reject");
    assert_eq!(
        frame_snapshot(&engine),
        before,
        "{label} mutated the engine"
    );
}

fn assert_completed_parity(
    ordinary: &ApplicationEngine<NoNativeContractProvider>,
    specialized: &ApplicationEngine<NoNativeContractProvider>,
) {
    assert_eq!(specialized.state(), ordinary.state());
    assert_eq!(
        specialized.instructions_executed(),
        ordinary.instructions_executed()
    );
    assert_eq!(
        specialized.fee_consumed_pico(),
        ordinary.fee_consumed_pico()
    );
    assert_eq!(
        specialized.gas_consumed_pico(),
        ordinary.gas_consumed_pico()
    );
    assert_eq!(specialized.reference_count(), ordinary.reference_count());
    assert_eq!(
        specialized.invocation_stack().len(),
        ordinary.invocation_stack().len()
    );
    assert_stack_values_equal(&result_stack(specialized), &result_stack(ordinary));
}

#[test]
fn exact_internal_call_frame_matches_ordinary_stack_accounting_and_unload() {
    let mut ordinary = prepared_engine(PreparedCase::default());
    let mut specialized = prepared_engine(PreparedCase::default());

    assert_eq!(
        ordinary.execute_until_invocation_stack_depth(1),
        VMState::BREAK
    );
    assert!(try_prepared_frame(&mut specialized));

    assert_eq!(specialized.state(), ordinary.state());
    assert_eq!(
        specialized.instructions_executed(),
        ordinary.instructions_executed()
    );
    assert_eq!(
        specialized.fee_consumed_pico(),
        ordinary.fee_consumed_pico()
    );
    assert_eq!(
        specialized.gas_consumed_pico(),
        ordinary.gas_consumed_pico()
    );
    assert_eq!(specialized.reference_count(), ordinary.reference_count());
    assert_eq!(specialized.invocation_stack().len(), 1);
    assert_eq!(
        specialized.current_script_hash(),
        ordinary.current_script_hash()
    );
    assert_eq!(
        specialized.get_calling_script_hash(),
        ordinary.get_calling_script_hash()
    );
    assert_stack_values_equal(&current_stack(&specialized), &current_stack(&ordinary));
    let specialized_result = current_stack(&specialized)
        .into_iter()
        .next()
        .expect("pair key remains on shared caller stack");
    assert_eq!(specialized_result.stack_item_type(), StackItemType::Buffer);
}

#[test]
fn exact_shadow_route_applies_once_and_matches_complete_ordinary_run() {
    let mut ordinary = prepared_engine(PreparedCase::default());
    let mut specialized = prepared_engine(PreparedCase::default());
    let ordinary_state = ordinary.execute_allow_fault();
    let result = specialized.execute_flamingo_shadow_candidate(&shadow_control());

    assert_eq!(ordinary_state, VMState::HALT);
    assert_eq!(result.state, ordinary_state);
    assert_eq!(result.applied_frames, 1);
    assert_completed_parity(&ordinary, &specialized);
}

#[test]
fn exact_shadow_route_preserves_a_later_outer_assert_fault() {
    let case = PreparedCase {
        outer_fault: true,
        ..PreparedCase::default()
    };
    let mut ordinary = prepared_engine(case.clone());
    let mut specialized = prepared_engine(case);

    let ordinary_state = ordinary.execute_allow_fault();
    let result = specialized.execute_flamingo_shadow_candidate(&shadow_control());

    assert_eq!(ordinary_state, VMState::FAULT);
    assert_eq!(result.state, ordinary_state);
    assert_eq!(result.applied_frames, 1);
    assert_completed_parity(&ordinary, &specialized);

    let expected = "ASSERT is executed with false result. [ip=3 opcode=ASSERT eval_depth=0]";
    assert_eq!(ordinary.fault_exception(), Some(expected));
    assert_eq!(specialized.fault_exception(), Some(expected));
    assert_eq!(
        specialized.uncaught_exception_item(),
        ordinary.uncaught_exception_item()
    );
    assert_eq!(
        specialized
            .uncaught_exception_item()
            .expect("candidate fault exception")
            .as_bytes()
            .expect("fault is a ByteString")
            .as_slice(),
        expected.as_bytes()
    );
}

#[test]
fn identity_context_and_argument_mismatches_reject_before_mutation() {
    let mut wrong_network = PreparedCase::default();
    assert_eq!(wrong_network.settings.network, MAINNET_MAGIC);
    wrong_network.settings.network ^= 1;

    let wrong_script = PreparedCase {
        wrong_script: true,
        ..PreparedCase::default()
    };
    let wrong_update = PreparedCase {
        contract_update: CONTRACT_UPDATE + 1,
        ..PreparedCase::default()
    };
    let wrong_frame = PreparedCase {
        shared_frame: false,
        ..PreparedCase::default()
    };
    let wrong_static = PreparedCase {
        static_prefix: Some(StackItem::from_buffer(vec![0xFE])),
        ..PreparedCase::default()
    };
    let wrong_type = PreparedCase {
        token_a: StackItem::from_buffer(vec![0x11; 20]),
        ..PreparedCase::default()
    };
    let fee_whitelisted = PreparedCase {
        fee_whitelisted: true,
        ..PreparedCase::default()
    };
    let profiled = PreparedCase {
        profiling: true,
        ..PreparedCase::default()
    };

    for (label, case) in [
        ("network", wrong_network),
        ("script", wrong_script),
        ("contract update", wrong_update),
        ("internal CALL frame", wrong_frame),
        ("static field", wrong_static),
        ("argument type", wrong_type),
        ("fee whitelist", fee_whitelisted),
        ("VM profile", profiled),
    ] {
        assert_rejected_without_mutation(label, prepared_engine(case));
    }
}

#[test]
fn contract_update_and_static_state_changes_execute_fully_through_ordinary_vm() {
    for (label, case) in [
        (
            "contract update",
            PreparedCase {
                contract_update: CONTRACT_UPDATE + 1,
                ..PreparedCase::default()
            },
        ),
        (
            "static state",
            PreparedCase {
                static_prefix: Some(StackItem::from_buffer(vec![0xFE])),
                ..PreparedCase::default()
            },
        ),
    ] {
        let mut ordinary = prepared_engine(case.clone());
        let mut routed = prepared_engine(case);
        let ordinary_state = ordinary.execute_allow_fault();
        let result = routed.execute_flamingo_shadow_candidate(&shadow_control());

        assert_eq!(ordinary_state, VMState::HALT, "ordinary {label}");
        assert_eq!(result.state, ordinary_state, "routed {label}");
        assert_eq!(result.applied_frames, 0, "routed {label}");
        assert_completed_parity(&ordinary, &routed);
    }
}

#[test]
fn insufficient_gas_and_transient_reference_limit_reject_before_mutation() {
    let artifact = try_flamingo_pair_key(
        &[
            StackItem::from_byte_string(vec![0x11; 20]),
            StackItem::from_byte_string(vec![0x22; 20]),
        ],
        &StackItem::from_buffer(vec![0xFF]),
        false,
        false,
    )
    .expect("eligible branch");
    let insufficient_gas = PreparedCase {
        gas_limit: i64::try_from(artifact.gas_units()).expect("small gas") * 30 - 1,
        ..PreparedCase::default()
    };
    assert_rejected_without_mutation(
        "insufficient aggregate gas",
        prepared_engine(insufficient_gas),
    );

    let near_stack_limit = PreparedCase {
        filler_items: 2_041,
        ..PreparedCase::default()
    };
    let engine = prepared_engine(near_stack_limit);
    let references = engine.reference_count();
    let maximum = engine.execution_limits().max_stack_size as usize;
    assert!(references <= maximum);
    assert!(references + 3 > maximum);
    assert_rejected_without_mutation("ordinary transient reference peak", engine);
}

#[test]
fn global_and_candidate_kill_switches_keep_the_route_ordinary() {
    let mut ordinary = prepared_engine(PreparedCase::default());
    assert_eq!(ordinary.execute_allow_fault(), VMState::HALT);

    let global = shadow_control();
    global.kill_global();
    let mut global_engine = prepared_engine(PreparedCase::default());
    let global_result = global_engine.execute_flamingo_shadow_candidate(&global);
    assert_eq!(global_result.applied_frames, 0);
    assert_completed_parity(&ordinary, &global_engine);

    let candidate = exact_candidate();
    let candidate_control = shadow_control();
    assert!(candidate_control.kill_candidate(
        FLAMINGO_FACTORY_PAIR_KEY_CANDIDATE_ID,
        candidate.identity().candidate_version(),
    ));
    let mut candidate_engine = prepared_engine(PreparedCase::default());
    let candidate_result = candidate_engine.execute_flamingo_shadow_candidate(&candidate_control);
    assert_eq!(candidate_result.applied_frames, 0);
    assert_completed_parity(&ordinary, &candidate_engine);
}

#[test]
fn prepared_candidate_cache_reuses_only_the_exact_hardfork_identity() {
    let first = HardforkTableIdentity::unconfigured();
    let second = first.with_state(
        neo_primitives::Hardfork::HfAspidochelone,
        HardforkPlanState::Active {
            activation_height: 1_730_000,
        },
    );
    let first_prepared = prepared_flamingo_candidate(first).expect("first prepared candidate");
    let first_again = prepared_flamingo_candidate(first).expect("cached prepared candidate");
    let second_prepared = prepared_flamingo_candidate(second).expect("second prepared candidate");

    assert!(Arc::ptr_eq(&first_prepared, &first_again));
    assert!(!Arc::ptr_eq(&first_prepared, &second_prepared));
    assert_ne!(
        first_prepared.candidate.identity().execution().hardforks(),
        second_prepared.candidate.identity().execution().hardforks()
    );
}

#[test]
fn exact_shadow_route_matches_at_every_mainnet_hardfork_boundary() {
    let settings = ProtocolSettings::default();
    let mut heights = settings
        .hardforks
        .activation_heights()
        .flat_map(|height| [height.saturating_sub(1), height])
        .collect::<Vec<_>>();
    heights.push(0);
    heights.sort_unstable();
    heights.dedup();

    for height in heights {
        let case = PreparedCase {
            settings: settings.clone(),
            profile_height: height,
            ..PreparedCase::default()
        };
        let mut ordinary = prepared_engine(case.clone());
        let mut specialized = prepared_engine(case);

        let ordinary_state = ordinary.execute_allow_fault();
        let result = specialized.execute_flamingo_shadow_candidate(&shadow_control());

        assert_eq!(ordinary_state, VMState::HALT, "ordinary height {height}");
        assert_eq!(result.state, ordinary_state, "candidate height {height}");
        assert_eq!(result.applied_frames, 1, "candidate height {height}");
        assert_completed_parity(&ordinary, &specialized);
    }
}

#[test]
#[ignore = "release-only prepared-route benchmark; run explicitly with --nocapture"]
fn benchmark_complete_prepared_route_against_ordinary_execution() {
    const BATCHES: usize = 40;
    const ENGINES_PER_BATCH: usize = 64;

    fn measure(specialized: bool, control: &SpecializationControl) -> std::time::Duration {
        let mut elapsed = std::time::Duration::ZERO;
        for _ in 0..BATCHES {
            let mut engines = (0..ENGINES_PER_BATCH)
                .map(|_| prepared_engine(PreparedCase::default()))
                .collect::<Vec<_>>();
            let started = std::time::Instant::now();
            for engine in &mut engines {
                if specialized {
                    let result = engine.execute_flamingo_shadow_candidate(control);
                    assert_eq!(result.state, VMState::HALT);
                    assert_eq!(result.applied_frames, 1);
                } else {
                    assert_eq!(engine.execute_allow_fault(), VMState::HALT);
                }
                std::hint::black_box(engine.fee_consumed_pico());
            }
            elapsed += started.elapsed();
        }
        elapsed
    }

    fn measure_prebuilt(
        control: &SpecializationControl,
        candidate: &CandidateContract,
        policy: &HostAccessPolicy,
    ) -> std::time::Duration {
        let mut elapsed = std::time::Duration::ZERO;
        for _ in 0..BATCHES {
            let mut engines = (0..ENGINES_PER_BATCH)
                .map(|_| prepared_engine(PreparedCase::default()))
                .collect::<Vec<_>>();
            let started = std::time::Instant::now();
            for engine in &mut engines {
                let result =
                    engine.execute_prepared_flamingo_shadow_candidate(control, candidate, policy);
                assert_eq!(result.state, VMState::HALT);
                assert_eq!(result.applied_frames, 1);
                std::hint::black_box(engine.fee_consumed_pico());
            }
            elapsed += started.elapsed();
        }
        elapsed
    }

    let control = shadow_control();
    let identity_probe = prepared_engine(PreparedCase::default());
    let candidate = flamingo_pair_key_candidate(identity_probe.hardfork_plan_identity())
        .expect("benchmark candidate");
    let policy = flamingo_cpu_fee_policy(&candidate).expect("benchmark fee policy");
    let _ = measure(false, &control);
    let _ = measure(true, &control);
    let _ = measure_prebuilt(&control, &candidate, &policy);
    let ordinary = measure(false, &control);
    let cached = measure(true, &control);
    let prebuilt = measure_prebuilt(&control, &candidate, &policy);
    let samples = (BATCHES * ENGINES_PER_BATCH) as f64;
    let ordinary_ns = ordinary.as_nanos() as f64 / samples;
    let cached_ns = cached.as_nanos() as f64 / samples;
    let prebuilt_ns = prebuilt.as_nanos() as f64 / samples;
    eprintln!(
        "prepared Flamingo route: ordinary={ordinary_ns:.2} ns cached={cached_ns:.2} ns prebuilt={prebuilt_ns:.2} ns cached_speedup={:.3}x prebuilt_speedup={:.3}x samples={samples}",
        ordinary_ns / cached_ns,
        ordinary_ns / prebuilt_ns,
    );
}
