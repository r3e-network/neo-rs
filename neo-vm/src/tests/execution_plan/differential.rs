use super::*;
use crate::{
    ExecutionEngine, HardforkPlanState, JumpTable, OpCode, Script, StackItemRpcJson, StackItemType,
    VmExecutionProfile, VmState,
};
use neo_primitives::{Hardfork, TriggerType};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Copy)]
enum TableEra {
    PreEchidna,
    Echidna,
    Gorgon,
}

impl TableEra {
    const ALL: [Self; 3] = [Self::PreEchidna, Self::Echidna, Self::Gorgon];

    fn jump_table(self) -> JumpTable<()> {
        match self {
            Self::PreEchidna => JumpTable::not_echidna(),
            Self::Echidna => JumpTable::not_gorgon(),
            Self::Gorgon => JumpTable::new(),
        }
    }

    fn hardforks(self) -> HardforkTableIdentity {
        let table = HardforkTableIdentity::unconfigured().with_state(
            Hardfork::HfEchidna,
            match self {
                Self::PreEchidna => HardforkPlanState::Pending {
                    activation_height: 1,
                },
                Self::Echidna | Self::Gorgon => HardforkPlanState::Active {
                    activation_height: 1,
                },
            },
        );
        table.with_state(
            Hardfork::HfGorgon,
            match self {
                Self::Gorgon => HardforkPlanState::Active {
                    activation_height: 2,
                },
                Self::PreEchidna | Self::Echidna => HardforkPlanState::Pending {
                    activation_height: 2,
                },
            },
        )
    }
}

#[derive(Debug, Eq, PartialEq)]
struct Outcome {
    state: VmState,
    instructions: u64,
    gas: u64,
    result: Vec<Value>,
    exception: Option<Value>,
    references: usize,
    profile: VmExecutionProfile,
}

fn plan_key(script: &[u8], era: TableEra) -> ExecutionPlanKey {
    ExecutionPlanKey::new(
        Arc::<[u8]>::from(script),
        0,
        ProtocolIdentity::new(0x334f_454e, ProtocolVersion::NEO_N3_V3_10_1),
        era.hardforks(),
        TriggerType::APPLICATION,
        None,
    )
}

fn execute(script: &[u8], era: TableEra, planned: bool) -> Outcome {
    let mut engine = ExecutionEngine::new(Some(era.jump_table()));
    engine.enable_execution_profiling();
    if planned {
        let plan = Arc::new(
            ExecutionPlan::build(plan_key(script, era), ExecutionPlanLimits::default())
                .expect("strict differential plan"),
        );
        engine
            .load_script_with_plan(Script::new_relaxed(script.to_vec()), plan, -1, 0)
            .expect("load planned differential script");
    } else {
        engine
            .load_script(Script::new_relaxed(script.to_vec()), -1, 0)
            .expect("load ordinary differential script");
    }
    let state = engine.execute();
    let result = engine
        .result_stack()
        .to_vec()
        .iter()
        .map(|item| StackItemRpcJson::stack_item_rpc_json(item, None).expect("render result"))
        .collect();
    let exception = engine
        .uncaught_exception()
        .map(|item| StackItemRpcJson::stack_item_rpc_json(item, None).expect("render exception"));
    Outcome {
        state,
        instructions: engine.instructions_executed,
        gas: engine.gas_consumed(),
        result,
        exception,
        references: engine.reference_counter().count(),
        profile: engine.execution_profile().expect("profile enabled"),
    }
}

fn assert_parity(script: &[u8], era: TableEra) {
    assert_eq!(execute(script, era, true), execute(script, era, false));
}

fn opcode_script(opcode: OpCode) -> Vec<u8> {
    use OpCode::{
        CALL, CALL_L, ENDTRY, ENDTRY_L, JMP, JMP_L, JMPEQ, JMPEQ_L, JMPGE, JMPGE_L, JMPGT, JMPGT_L,
        JMPIF, JMPIF_L, JMPIFNOT, JMPIFNOT_L, JMPLE, JMPLE_L, JMPLT, JMPLT_L, JMPNE, JMPNE_L,
        NEWARRAY_T, PUSHA, PUSHDATA1, PUSHDATA2, PUSHDATA4, SYSCALL, TRY, TRY_L,
    };

    let mut script = vec![opcode.byte()];
    match opcode {
        PUSHDATA1 => script.push(0),
        PUSHDATA2 => script.extend_from_slice(&0u16.to_le_bytes()),
        PUSHDATA4 => script.extend_from_slice(&0u32.to_le_bytes()),
        JMP | JMPIF | JMPIFNOT | JMPEQ | JMPNE | JMPGT | JMPGE | JMPLT | JMPLE | CALL | ENDTRY => {
            script.push(2)
        }
        JMP_L | JMPIF_L | JMPIFNOT_L | JMPEQ_L | JMPNE_L | JMPGT_L | JMPGE_L | JMPLT_L
        | JMPLE_L | CALL_L | ENDTRY_L | PUSHA => script.extend_from_slice(&5i32.to_le_bytes()),
        TRY => script.extend_from_slice(&[3, 0]),
        TRY_L => {
            script.extend_from_slice(&9i32.to_le_bytes());
            script.extend_from_slice(&0i32.to_le_bytes());
        }
        NEWARRAY_T | OpCode::ISTYPE | OpCode::CONVERT => {
            script.push(StackItemType::Integer.to_byte());
        }
        SYSCALL => script.extend_from_slice(&0u32.to_le_bytes()),
        _ => script.extend(std::iter::repeat_n(0, opcode.operand_size())),
    }
    script.push(OpCode::RET.byte());
    script
}

#[test]
fn every_opcode_matches_across_v3101_jump_table_eras() {
    for era in TableEra::ALL {
        for opcode in OpCode::ALL {
            let script = opcode_script(opcode);
            assert_parity(&script, era);
        }
    }
}

fn next_random(state: &mut u64) -> u64 {
    *state = state
        .wrapping_mul(6_364_136_223_846_793_005)
        .wrapping_add(1_442_695_040_888_963_407);
    *state
}

fn randomized_balanced_script(seed: u64) -> Vec<u8> {
    const BINARY: [OpCode; 13] = [
        OpCode::ADD,
        OpCode::SUB,
        OpCode::MUL,
        OpCode::AND,
        OpCode::OR,
        OpCode::XOR,
        OpCode::BOOLAND,
        OpCode::BOOLOR,
        OpCode::NUMEQUAL,
        OpCode::NUMNOTEQUAL,
        OpCode::LT,
        OpCode::MAX,
        OpCode::MIN,
    ];
    const UNARY: [OpCode; 7] = [
        OpCode::INC,
        OpCode::DEC,
        OpCode::NEGATE,
        OpCode::ABS,
        OpCode::NOT,
        OpCode::NZ,
        OpCode::SIGN,
    ];

    let mut random = seed;
    let mut depth = 0usize;
    let mut script = Vec::with_capacity(129);
    for _ in 0..128 {
        let choice = next_random(&mut random) as usize;
        if depth < 2 || choice.is_multiple_of(5) {
            script.push(OpCode::PUSH0.byte() + (choice % 17) as u8);
            depth += 1;
        } else if choice.is_multiple_of(7) {
            script.push(OpCode::DUP.byte());
            depth += 1;
        } else if choice.is_multiple_of(11) && depth > 1 {
            script.push(OpCode::DROP.byte());
            depth -= 1;
        } else if choice & 1 == 0 {
            script.push(BINARY[choice % BINARY.len()].byte());
            depth -= 1;
        } else {
            script.push(UNARY[choice % UNARY.len()].byte());
        }
    }
    script.push(OpCode::RET.byte());
    script
}

#[test]
fn deterministic_randomized_programs_match_with_diagnostics() {
    for seed in 0..512 {
        let script = randomized_balanced_script(seed);
        let era = TableEra::ALL[seed as usize % TableEra::ALL.len()];
        assert_parity(&script, era);
    }
}

#[test]
fn explicit_calls_exceptions_and_fault_boundaries_match() {
    let scripts = [
        vec![
            OpCode::CALL.byte(),
            3,
            OpCode::RET.byte(),
            OpCode::PUSH4.byte(),
            OpCode::RET.byte(),
        ],
        vec![
            OpCode::TRY.byte(),
            6,
            0,
            OpCode::PUSH1.byte(),
            OpCode::THROW.byte(),
            OpCode::RET.byte(),
            OpCode::DROP.byte(),
            OpCode::PUSH2.byte(),
            OpCode::ENDTRY.byte(),
            2,
            OpCode::RET.byte(),
        ],
        vec![
            OpCode::PUSH0.byte(),
            OpCode::ASSERT.byte(),
            OpCode::RET.byte(),
        ],
        vec![
            OpCode::PUSH1.byte(),
            OpCode::PUSH0.byte(),
            OpCode::DIV.byte(),
        ],
        vec![OpCode::DROP.byte(), OpCode::RET.byte()],
    ];
    for era in TableEra::ALL {
        for script in &scripts {
            assert_parity(script, era);
        }
    }
}
