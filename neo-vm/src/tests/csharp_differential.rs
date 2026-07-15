use std::collections::BTreeSet;

use serde_json::Value;

use crate::{ExecutionEngine, OpCode, Script, StackItem, StackItemType};

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/csharp-v3.10.1-vm.json"
));
const NEO_VM_COMMIT: &str = "004cd6070a940405818d9357638277dd44407e2e";

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("valid C# Neo.VM fixture")
}

fn case<'a>(fixture: &'a Value, id: &str) -> &'a Value {
    fixture["cases"]
        .as_array()
        .expect("fixture cases")
        .iter()
        .find(|case| case["id"] == id)
        .unwrap_or_else(|| panic!("missing C# fixture case {id}"))
}

fn observed_usize(case: &Value, field: &str) -> usize {
    case["observed"][field]
        .as_u64()
        .unwrap_or_else(|| panic!("{field} must be an unsigned integer")) as usize
}

fn execute(script: Vec<u8>, rvcount: i32) -> ExecutionEngine {
    let mut engine = ExecutionEngine::<()>::new(None);
    engine
        .load_script(Script::new_relaxed(script), rvcount, 0)
        .expect("load fixture script");
    engine.execute();
    engine
}

fn assert_engine(case: &Value, engine: &ExecutionEngine) {
    assert_eq!(
        format!("{:?}", engine.state()).to_ascii_uppercase(),
        case["observed"]["state"].as_str().expect("C# state")
    );
    assert_eq!(
        engine.invocation_stack().len(),
        observed_usize(case, "invocation_stack_depth")
    );
    assert_eq!(
        engine.result_stack().len(),
        observed_usize(case, "result_stack_depth")
    );
}

#[test]
fn csharp_v3101_fixture_has_pinned_complete_coverage() {
    let fixture = fixture();
    assert_eq!(fixture["schema"], 1);
    assert_eq!(fixture["oracle"]["commit"], NEO_VM_COMMIT);
    assert_eq!(fixture["oracle"]["version"], "3.10.1");

    let actual: BTreeSet<_> = fixture["cases"]
        .as_array()
        .expect("fixture cases")
        .iter()
        .map(|case| case["id"].as_str().expect("case id"))
        .collect();
    let expected = BTreeSet::from([
        "abortmsg_invalid_utf8",
        "abortmsg_valid_utf8",
        "call_to_script_end",
        "context_at_script_end",
        "context_beyond_script_end",
        "endtry_target_beyond_script_end",
        "implicit_ret_exact",
        "implicit_ret_too_few",
        "implicit_ret_too_many",
        "invalid_argument_index_preserves_operand",
        "invalid_local_index_preserves_operand",
        "invalid_slot_store_preserves_operand",
        "invalid_static_index_preserves_operand",
        "jump_to_script_end",
        "null_convert_interopinterface",
        "null_convert_map",
        "null_convert_pointer",
        "relaxed_unreachable_malformed",
        "strict_convert_any",
        "strict_jump_to_end",
        "strict_unreachable_malformed",
        "try_target_beyond_script_end",
        "unhandled_throw_preserves_frames",
    ]);
    assert_eq!(actual, expected);
}

#[test]
fn csharp_v3101_return_and_script_mode_fixtures_match() {
    let fixture = fixture();

    let exact = execute(vec![OpCode::PUSH1.byte()], 1);
    let exact_case = case(&fixture, "implicit_ret_exact");
    assert_engine(exact_case, &exact);
    assert_eq!(
        exact
            .result_stack()
            .peek(0)
            .expect("return item")
            .as_int()
            .expect("integer return")
            .to_string(),
        exact_case["observed"]["result_stack"][0]["value"]
            .as_str()
            .expect("C# return value")
    );

    let too_few = execute(Vec::new(), 1);
    assert_engine(case(&fixture, "implicit_ret_too_few"), &too_few);
    let too_many = execute(vec![OpCode::PUSH1.byte()], 0);
    assert_engine(case(&fixture, "implicit_ret_too_many"), &too_many);

    let relaxed = execute(vec![OpCode::RET.byte(), 0xff], -1);
    assert_engine(case(&fixture, "relaxed_unreachable_malformed"), &relaxed);
    assert!(Script::new(vec![OpCode::RET.byte(), 0xff], true).is_err());
    assert!(Script::new(vec![OpCode::JMP.byte(), 2], true).is_err());
    assert!(
        Script::new(
            vec![OpCode::CONVERT.byte(), StackItemType::Any.to_byte()],
            true
        )
        .is_err()
    );
}

#[test]
fn csharp_v3101_control_flow_boundary_fixtures_match() {
    let fixture = fixture();
    let script = Script::new_relaxed(vec![OpCode::RET.byte()]);

    let mut at_end = ExecutionEngine::<()>::new(None);
    assert!(at_end.load_script(script.clone(), 0, script.len()).is_ok());
    assert_eq!(
        case(&fixture, "context_at_script_end")["observed"]["outcome"],
        "ok"
    );

    let mut beyond = ExecutionEngine::<()>::new(None);
    assert!(
        beyond
            .load_script(script.clone(), 0, script.len() + 1)
            .is_err()
    );
    assert_eq!(
        case(&fixture, "context_beyond_script_end")["observed"]["outcome"],
        "error"
    );

    let mut call = ExecutionEngine::<()>::new(None);
    call.load_script(script.clone(), 0, 0)
        .expect("load CALL fixture");
    call.execute_call(script.len()).expect("CALL to script end");
    call.execute();
    assert_engine(case(&fixture, "call_to_script_end"), &call);

    let mut jump = ExecutionEngine::<()>::new(None);
    jump.load_script(script.clone(), -1, 0)
        .expect("load JMP fixture");
    assert!(jump.execute_jump(script.len() as i32).is_err());

    let mut invalid_catch = ExecutionEngine::<()>::new(None);
    invalid_catch
        .load_script(script.clone(), -1, 0)
        .expect("load TRY fixture");
    invalid_catch.execute_try(2, 0).expect("install TRY");
    assert!(invalid_catch.execute_throw(Some(StackItem::Null)).is_err());
    assert_eq!(
        invalid_catch.invocation_stack().len(),
        observed_usize(
            case(&fixture, "try_target_beyond_script_end"),
            "invocation_stack_depth"
        )
    );

    let mut invalid_end = ExecutionEngine::<()>::new(None);
    invalid_end
        .load_script(script, -1, 0)
        .expect("load ENDTRY fixture");
    invalid_end.execute_try(1, 0).expect("install TRY");
    assert!(invalid_end.execute_end_try(2).is_err());
    assert_eq!(
        invalid_end.invocation_stack().len(),
        observed_usize(
            case(&fixture, "endtry_target_beyond_script_end"),
            "invocation_stack_depth"
        )
    );
}

#[test]
fn csharp_v3101_value_fault_and_builder_fixtures_match() {
    let fixture = fixture();

    for (id, item_type) in [
        ("null_convert_map", StackItemType::Map),
        ("null_convert_pointer", StackItemType::Pointer),
        (
            "null_convert_interopinterface",
            StackItemType::InteropInterface,
        ),
    ] {
        let converted = StackItem::Null
            .convert_to(item_type)
            .expect("C# Null conversion succeeds");
        assert!(converted.is_null());
        assert_eq!(
            format!("{:?}", converted.stack_item_type()),
            case(&fixture, id)["observed"]["result_type"]
                .as_str()
                .expect("C# result type")
        );
    }

    for (id, script) in [
        (
            "invalid_slot_store_preserves_operand",
            vec![OpCode::PUSH1.byte(), OpCode::STLOC0.byte()],
        ),
        (
            "invalid_static_index_preserves_operand",
            vec![
                OpCode::INITSSLOT.byte(),
                1,
                OpCode::PUSH1.byte(),
                OpCode::STSFLD.byte(),
                1,
            ],
        ),
        (
            "invalid_local_index_preserves_operand",
            vec![
                OpCode::INITSLOT.byte(),
                1,
                0,
                OpCode::PUSH1.byte(),
                OpCode::STLOC.byte(),
                1,
            ],
        ),
        (
            "invalid_argument_index_preserves_operand",
            vec![
                OpCode::PUSH1.byte(),
                OpCode::INITSLOT.byte(),
                0,
                1,
                OpCode::PUSH2.byte(),
                OpCode::STARG.byte(),
                1,
            ],
        ),
    ] {
        let slot = execute(script, -1);
        let slot_case = case(&fixture, id);
        assert_engine(slot_case, &slot);
        assert_eq!(
            slot.current_context()
                .expect("faulting slot context")
                .evaluation_stack()
                .len(),
            observed_usize(slot_case, "evaluation_stack_depth"),
            "{id}"
        );
    }

    let mut throwing = ExecutionEngine::<()>::new(None);
    throwing
        .load_script(Script::new_relaxed(vec![OpCode::RET.byte()]), 0, 0)
        .expect("load outer frame");
    throwing
        .load_script(
            Script::new_relaxed(vec![OpCode::PUSH1.byte(), OpCode::THROW.byte()]),
            -1,
            0,
        )
        .expect("load throwing frame");
    throwing.execute();
    let throw_case = case(&fixture, "unhandled_throw_preserves_frames");
    assert_engine(throw_case, &throwing);
    assert_eq!(
        throwing
            .current_context()
            .expect("throwing context")
            .evaluation_stack()
            .len(),
        observed_usize(throw_case, "current_evaluation_stack_depth")
    );

    let valid_abort = execute(
        vec![
            OpCode::PUSHDATA1.byte(),
            3,
            b'N',
            b'E',
            b'O',
            OpCode::ABORTMSG.byte(),
        ],
        -1,
    );
    assert_engine(case(&fixture, "abortmsg_valid_utf8"), &valid_abort);
    let invalid_abort = execute(
        vec![OpCode::PUSHDATA1.byte(), 1, 0xff, OpCode::ABORTMSG.byte()],
        -1,
    );
    assert_engine(case(&fixture, "abortmsg_invalid_utf8"), &invalid_abort);

    let mut builder = crate::script_builder::ScriptBuilder::new();
    builder
        .emit_push_stack_item(&StackItem::from_struct(vec![
            StackItem::from_i64(1),
            StackItem::from_i64(2),
        ]))
        .expect("emit struct");
    assert_eq!(
        builder.to_array(),
        vec![
            OpCode::PUSH2.byte(),
            OpCode::PUSH1.byte(),
            OpCode::PUSH2.byte(),
            OpCode::PACKSTRUCT.byte(),
        ]
    );
}
