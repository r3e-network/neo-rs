use super::*;
use crate::{ContractResolutionIdentity, HardforkTableIdentity, ProtocolIdentity, ProtocolVersion};
use neo_primitives::{TriggerType, UInt160};
use std::sync::Arc;

fn key(script: Vec<u8>, entry: u32) -> ExecutionPlanKey {
    ExecutionPlanKey::new(
        Arc::<[u8]>::from(script),
        entry,
        ProtocolIdentity::new(0x334f_454e, ProtocolVersion::NEO_N3_V3_10_1),
        HardforkTableIdentity::unconfigured(),
        TriggerType::APPLICATION,
        Some(ContractResolutionIdentity::new(
            UInt160::from([0x42; 20]),
            17,
            3,
            0x1020_3040,
        )),
    )
}

fn build(script: Vec<u8>, entry: u32) -> ExecutionPlan {
    ExecutionPlan::build(key(script, entry), ExecutionPlanLimits::default()).expect("valid plan")
}

#[test]
fn plan_builds_direct_offsets_and_basic_blocks() {
    let script = vec![
        OpCode::PUSH1.byte(),
        OpCode::JMPIF.byte(),
        3,
        OpCode::PUSH2.byte(),
        OpCode::RET.byte(),
    ];
    let plan = build(script.clone(), 0);

    assert_eq!(plan.instructions().len(), 4);
    assert_eq!(
        plan.instruction_at(0).unwrap().instruction().opcode(),
        OpCode::PUSH1
    );
    assert_eq!(
        plan.instruction_at(1).unwrap().control_flow(),
        PlannedControlFlow::Branch {
            target: 4,
            fallthrough: 3,
        }
    );
    assert!(plan.instruction_at(2).is_none());
    assert!(plan.matches_script(plan.key().script_hash(), &script));
    assert_eq!(
        plan.basic_blocks(),
        &[
            BasicBlock {
                start_ip: 0,
                end_ip: 3,
                first_instruction: 0,
                instruction_count: 2,
            },
            BasicBlock {
                start_ip: 3,
                end_ip: 4,
                first_instruction: 2,
                instruction_count: 1,
            },
            BasicBlock {
                start_ip: 4,
                end_ip: 5,
                first_instruction: 3,
                instruction_count: 1,
            },
        ]
    );
}

#[test]
fn plan_resolves_calls_try_targets_syscalls_and_pusha() {
    let script = vec![
        OpCode::CALL.byte(),
        3,
        OpCode::RET.byte(),
        OpCode::TRY.byte(),
        8,
        0,
        OpCode::SYSCALL.byte(),
        0x44,
        0x33,
        0x22,
        0x11,
        OpCode::ENDTRY.byte(),
        2,
        OpCode::RET.byte(),
        OpCode::PUSHA.byte(),
        0xf2,
        0xff,
        0xff,
        0xff,
        OpCode::RET.byte(),
    ];
    let plan = build(script, 0);

    assert_eq!(
        plan.instruction_at(0).unwrap().control_flow(),
        PlannedControlFlow::Call {
            target: 3,
            return_ip: 2,
        }
    );
    assert_eq!(
        plan.instruction_at(3).unwrap().control_flow(),
        PlannedControlFlow::Try {
            catch_target: Some(11),
            finally_target: None,
            fallthrough: 6,
        }
    );
    assert_eq!(
        plan.instruction_at(6).unwrap().control_flow(),
        PlannedControlFlow::Syscall {
            service: 0x1122_3344,
        }
    );
    assert_eq!(plan.instruction_at(14).unwrap().address_target(), Some(0));
}

#[test]
fn empty_script_supports_implicit_ret_entry_at_end() {
    let plan = build(Vec::new(), 0);
    assert!(plan.instructions().is_empty());
    assert!(plan.basic_blocks().is_empty());
    assert!(plan.instruction_at(0).is_none());
}

#[test]
fn plan_rejects_non_instruction_entry_and_invalid_control_flow() {
    let operand_entry = key(
        vec![OpCode::PUSHDATA1.byte(), 1, 0x42, OpCode::RET.byte()],
        1,
    );
    assert!(matches!(
        ExecutionPlan::build(operand_entry, ExecutionPlanLimits::default()),
        Err(ExecutionPlanBuildError::EntryNotInstruction { entry: 1 })
    ));

    let invalid_jump = key(vec![OpCode::JMP.byte(), 1], 0);
    assert!(matches!(
        ExecutionPlan::build(invalid_jump, ExecutionPlanLimits::default()),
        Err(ExecutionPlanBuildError::InvalidScript(_))
    ));
}

#[test]
fn every_construction_limit_fails_closed() {
    let script = vec![OpCode::NOP.byte(), OpCode::RET.byte()];
    let cases = [
        (
            ExecutionPlanLimits {
                max_script_bytes: 1,
                ..ExecutionPlanLimits::default()
            },
            "script",
        ),
        (
            ExecutionPlanLimits {
                max_instructions: 1,
                ..ExecutionPlanLimits::default()
            },
            "instructions",
        ),
        (
            ExecutionPlanLimits {
                max_basic_blocks: 0,
                ..ExecutionPlanLimits::default()
            },
            "basic blocks",
        ),
        (
            ExecutionPlanLimits {
                max_plan_bytes: 1,
                ..ExecutionPlanLimits::default()
            },
            "bytes",
        ),
    ];

    for (limits, label) in cases {
        assert!(
            ExecutionPlan::build(key(script.clone(), 0), limits).is_err(),
            "{label} limit must reject the plan"
        );
    }
}

#[test]
fn entry_at_script_end_is_valid_but_beyond_end_is_not() {
    let script = vec![OpCode::RET.byte()];
    assert!(ExecutionPlan::build(key(script.clone(), 1), ExecutionPlanLimits::default()).is_ok());
    assert!(matches!(
        ExecutionPlan::build(key(script, 2), ExecutionPlanLimits::default()),
        Err(ExecutionPlanBuildError::EntryOutOfBounds {
            entry: 2,
            script_len: 1,
        })
    ));
}

#[derive(Debug, PartialEq)]
struct ExecutionOutcome {
    state: crate::VmState,
    instructions: u64,
    gas_consumed: u64,
    result_stack: Vec<crate::StackItem>,
    uncaught_exception: Option<crate::StackItem>,
    references: usize,
}

fn execute(script: &[u8], planned: bool) -> ExecutionOutcome {
    let mut engine = crate::ExecutionEngine::<()>::new(None);
    if planned {
        let plan = Arc::new(build(script.to_vec(), 0));
        engine
            .load_script_with_plan(crate::Script::new_relaxed(script.to_vec()), plan, -1, 0)
            .expect("load planned script");
    } else {
        engine
            .load_script(crate::Script::new_relaxed(script.to_vec()), -1, 0)
            .expect("load ordinary script");
    }
    let state = engine.execute();
    ExecutionOutcome {
        state,
        instructions: engine.instructions_executed,
        gas_consumed: engine.gas_consumed(),
        result_stack: engine.result_stack().to_vec(),
        uncaught_exception: engine.uncaught_exception().cloned(),
        references: engine.reference_counter().count(),
    }
}

#[test]
fn planned_executor_matches_ordinary_handlers_for_flow_and_faults() {
    let scripts = [
        vec![
            OpCode::PUSH1.byte(),
            OpCode::PUSH2.byte(),
            OpCode::ADD.byte(),
            OpCode::RET.byte(),
        ],
        vec![
            OpCode::PUSH1.byte(),
            OpCode::JMPIF.byte(),
            3,
            OpCode::PUSH2.byte(),
            OpCode::PUSH3.byte(),
            OpCode::RET.byte(),
        ],
        vec![
            OpCode::CALL.byte(),
            3,
            OpCode::RET.byte(),
            OpCode::PUSH4.byte(),
            OpCode::RET.byte(),
        ],
        vec![
            OpCode::PUSH0.byte(),
            OpCode::ASSERT.byte(),
            OpCode::RET.byte(),
        ],
    ];

    for script in scripts {
        assert_eq!(execute(&script, true), execute(&script, false));
    }
}

#[test]
fn planned_context_rejects_identity_mismatch_before_loading() {
    let mut engine = crate::ExecutionEngine::<()>::new(None);
    let plan = Arc::new(build(vec![OpCode::PUSH1.byte(), OpCode::RET.byte()], 0));
    let result = engine.load_script_with_plan(
        crate::Script::new_relaxed(vec![OpCode::PUSH2.byte(), OpCode::RET.byte()]),
        plan,
        -1,
        0,
    );

    assert!(result.is_err());
    assert!(engine.invocation_stack().is_empty());
    assert_eq!(engine.instructions_executed, 0);
}

#[test]
fn step_api_executes_exactly_one_instruction_on_planned_context() {
    let script = vec![
        OpCode::PUSH1.byte(),
        OpCode::PUSH2.byte(),
        OpCode::ADD.byte(),
        OpCode::RET.byte(),
    ];
    let plan = Arc::new(build(script.clone(), 0));
    let mut engine = crate::ExecutionEngine::<()>::new(None);
    engine
        .load_script_with_plan(crate::Script::new_relaxed(script), plan, -1, 0)
        .expect("load planned script");

    assert_eq!(engine.step_next(), crate::VmState::Break);
    assert_eq!(engine.instructions_executed, 1);
    assert_eq!(engine.current_context().unwrap().instruction_pointer(), 1);
}

#[test]
fn plan_selection_errors_and_mismatches_fall_back_before_loading() {
    let script = vec![OpCode::PUSH2.byte(), OpCode::RET.byte()];
    let mut failed_build = crate::ExecutionEngine::<()>::new(None);
    assert_eq!(
        failed_build
            .load_script_with_plan_fallback(
                crate::Script::new_relaxed(script.clone()),
                Err(crate::ExecutionPlanCacheError::ConstructionPanicked),
                -1,
                0,
            )
            .expect("ordinary fallback"),
        crate::ExecutionPlanRoute::OrdinaryFallback
    );
    assert!(
        failed_build
            .current_context()
            .unwrap()
            .execution_plan()
            .is_none()
    );
    assert_eq!(failed_build.execute(), crate::VmState::Halt);

    let mismatched = Arc::new(build(vec![OpCode::PUSH1.byte(), OpCode::RET.byte()], 0));
    let mut mismatch = crate::ExecutionEngine::<()>::new(None);
    assert_eq!(
        mismatch
            .load_script_with_plan_fallback(
                crate::Script::new_relaxed(script),
                Ok(mismatched),
                -1,
                0,
            )
            .expect("mismatch fallback"),
        crate::ExecutionPlanRoute::OrdinaryFallback
    );
    assert!(
        mismatch
            .current_context()
            .unwrap()
            .execution_plan()
            .is_none()
    );
}

#[test]
fn exact_plan_selection_reports_planned_route() {
    let script = vec![OpCode::PUSH1.byte(), OpCode::RET.byte()];
    let plan = Arc::new(build(script.clone(), 0));
    let mut engine = crate::ExecutionEngine::<()>::new(None);
    assert_eq!(
        engine
            .load_script_with_plan_fallback(crate::Script::new_relaxed(script), Ok(plan), -1, 0,)
            .expect("planned route"),
        crate::ExecutionPlanRoute::Planned
    );
    assert!(engine.current_context().unwrap().execution_plan().is_some());
}
