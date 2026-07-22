use std::collections::BTreeSet;

use serde_json::Value;

use super::*;
use neo_vm::OpCode;
use neo_vm::script_builder::ScriptBuilder;

const FIXTURE: &str = include_str!(concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/csharp-v3.10.1-application.json"
));
const NEO_COMMIT: &str = "d10e9ceecdabe3fcff719ee68ea5b76ba7e62c3d";

fn fixture() -> Value {
    serde_json::from_str(FIXTURE).expect("valid C# ApplicationEngine fixture")
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

fn engine(settings: ProtocolSettings) -> ApplicationEngine {
    ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        settings,
        TEST_MODE_GAS,
        NoDiagnostic,
        Arc::new(NoNativeContractProvider),
    )
    .expect("application engine")
}

fn hardfork_settings(entries: &[(Hardfork, u32)]) -> ProtocolSettings {
    let mut settings = ProtocolSettings::default();
    settings.hardforks =
        neo_config::HardforkSchedule::new().with_activations(entries.iter().copied());
    settings
}

fn handler_id(table: &JumpTable, opcode: OpCode) -> usize {
    table
        .get(opcode)
        .expect("opcode handler should be registered") as usize
}

#[test]
fn csharp_v3101_application_fixture_has_pinned_complete_coverage() {
    let fixture = fixture();
    assert_eq!(fixture["schema"], 1);
    assert_eq!(fixture["oracle"]["commit"], NEO_COMMIT);
    assert_eq!(fixture["oracle"]["version"], "3.10.1");

    let actual: BTreeSet<_> = fixture["cases"]
        .as_array()
        .expect("fixture cases")
        .iter()
        .map(|case| case["id"].as_str().expect("case id"))
        .collect();
    let expected = BTreeSet::from([
        "fault_clears_notifications",
        "jump_table_before_echidna",
        "jump_table_echidna_before_gorgon",
        "jump_table_gorgon_and_later",
        "runtime_load_script_convert_any_post_basilisk",
        "runtime_load_script_convert_any_pre_basilisk",
        "runtime_load_script_invalid_jump_post_basilisk",
        "runtime_load_script_invalid_jump_pre_basilisk",
        "script_builder_struct_uses_packstruct",
    ]);
    assert_eq!(actual, expected);
}

#[test]
fn csharp_v3101_runtime_load_script_is_strict_in_every_basilisk_era() {
    let fixture = fixture();
    for (era, basilisk_height) in [("pre_basilisk", 1), ("post_basilisk", 0)] {
        let settings = hardfork_settings(&[
            (Hardfork::HfBasilisk, basilisk_height),
            (Hardfork::HfEchidna, 2),
            (Hardfork::HfGorgon, 3),
        ]);
        for (kind, script) in [
            ("invalid_jump", vec![OpCode::JMP.byte(), 0x7f]),
            (
                "convert_any",
                vec![OpCode::CONVERT.byte(), neo_vm::StackItemType::Any.to_byte()],
            ),
        ] {
            let id = format!("runtime_load_script_{kind}_{era}");
            let oracle = case(&fixture, &id);
            let mut engine = engine(settings.clone());
            engine
                .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
                .expect("load calling context");

            assert_eq!(oracle["observed"]["outcome"], "error");
            assert!(
                engine
                    .runtime_load_script(script, CallFlags::READ_ONLY, Vec::new())
                    .is_err(),
                "{id} must reject the strict script"
            );
            assert_eq!(
                engine.invocation_stack().len(),
                observed_usize(oracle, "invocation_stack_depth")
            );
        }
    }
}

#[test]
fn csharp_v3101_fault_and_struct_fixtures_match() {
    let fixture = fixture();
    let notification_case = case(&fixture, "fault_clears_notifications");
    let mut engine = engine(ProtocolSettings::default());
    engine
        .load_script(vec![OpCode::ABORT.byte()], CallFlags::ALL, None)
        .expect("load faulting script");
    engine
        .send_notification(UInt160::zero(), "BeforeFault".to_string(), Vec::new())
        .expect("record pre-fault notification");
    assert_eq!(
        engine.notifications().len(),
        observed_usize(notification_case, "notifications_before")
    );
    assert_eq!(
        format!("{:?}", engine.execute_allow_fault()).to_ascii_uppercase(),
        notification_case["observed"]["state"]
            .as_str()
            .expect("C# state")
    );
    assert_eq!(
        engine.notifications().len(),
        observed_usize(notification_case, "notifications_after")
    );
    assert!(engine.fault_exception().is_some());

    let struct_case = case(&fixture, "script_builder_struct_uses_packstruct");
    let mut builder = ScriptBuilder::new();
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
    assert_eq!(
        struct_case["observed"]["last_opcode"]
            .as_str()
            .expect("C# opcode"),
        format!("{:?}", OpCode::PACKSTRUCT)
    );
    assert_eq!(struct_case["observed"]["script_hex"], "121112bf");
}

#[test]
fn csharp_v3101_hardfork_jump_table_fixtures_match() {
    let fixture = fixture();
    let opcodes = [
        OpCode::SUBSTR,
        OpCode::HASKEY,
        OpCode::PICKITEM,
        OpCode::SETITEM,
        OpCode::REMOVE,
        OpCode::SHL,
        OpCode::SHR,
    ];
    let eras = [
        (
            "jump_table_before_echidna",
            hardfork_settings(&[(Hardfork::HfEchidna, 1), (Hardfork::HfGorgon, 2)]),
            JumpTable::not_echidna(),
            [
                "VulnerableSubStr",
                "HasKey_Before543",
                "PickItem_Before543",
                "SetItem_Before543",
                "Remove_Before543",
                "VulnerableSHL",
                "VulnerableSHR",
            ],
        ),
        (
            "jump_table_echidna_before_gorgon",
            hardfork_settings(&[(Hardfork::HfEchidna, 0), (Hardfork::HfGorgon, 1)]),
            JumpTable::not_gorgon(),
            [
                "SubStr",
                "HasKey_Before543",
                "PickItem_Before543",
                "SetItem_Before543",
                "Remove_Before543",
                "VulnerableSHL",
                "VulnerableSHR",
            ],
        ),
        (
            "jump_table_gorgon_and_later",
            hardfork_settings(&[(Hardfork::HfEchidna, 0), (Hardfork::HfGorgon, 0)]),
            JumpTable::default(),
            [
                "SubStr", "HasKey", "PickItem", "SetItem", "Remove", "Shl", "Shr",
            ],
        ),
    ];

    for (id, settings, expected_table, csharp_names) in eras {
        let selected =
            ApplicationEngine::<NoNativeContractProvider>::select_jump_table(&settings, 0);
        let oracle = case(&fixture, id);
        for (opcode, csharp_name) in opcodes.iter().zip(csharp_names) {
            let opcode_name = format!("{opcode:?}");
            assert_eq!(
                oracle["observed"]["handlers"][opcode_name.as_str()]
                    .as_str()
                    .expect("C# handler name"),
                csharp_name
            );
            assert_eq!(
                handler_id(&selected, *opcode),
                handler_id(&expected_table, *opcode),
                "{id} {opcode:?} handler"
            );
        }
    }
}
