use super::*;
use crate::Diagnostic;
use crate::NativeContract;
use crate::NoDiagnostic;
use crate::application_engine::TEST_MODE_GAS;
use crate::native_contract_provider::{NativeContractProvider, NoNativeContractProvider};
use neo_config::ProtocolSettings;
use neo_primitives::TriggerType;
use neo_storage::{CacheRead, DataCache, StorageItem, StorageKey};
use neo_vm::Script;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::OpCode;
use neo_vm_rs::VmState as VMState;
use num_bigint::BigInt;
use std::sync::Arc;

const POLICY_CONTRACT_ID: i32 = -7;
const POLICY_PREFIX_EXEC_FEE_FACTOR: u8 = 18;

fn seed_policy_exec_fee_factor<B: CacheRead>(snapshot: &DataCache<B>, value: i64) {
    snapshot.add(
        StorageKey::new(POLICY_CONTRACT_ID, vec![POLICY_PREFIX_EXEC_FEE_FACTOR]),
        StorageItem::from_bytes(BigInt::from(value).to_signed_bytes_le()),
    );
}

struct TestPolicyContract {
    raw_exec_fee_factor: i64,
}

impl<P> NativeContract<P> for TestPolicyContract
where
    P: NativeContractProvider + 'static,
{
    fn id(&self) -> i32 {
        POLICY_CONTRACT_ID
    }

    fn hash(&self) -> UInt160 {
        UInt160::from_bytes(&[0xCC; 20]).expect("policy hash")
    }

    fn name(&self) -> &str {
        "PolicyContract"
    }

    fn methods(&self) -> &[crate::NativeMethod] {
        &[]
    }

    fn invoke<D, B>(
        &self,
        _engine: &mut ApplicationEngine<P, D, B>,
        method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>>
    where
        D: Diagnostic + 'static,
        B: CacheRead,
    {
        match method {
            "getExecPicoFeeFactor" => {
                Ok(BigInt::from(self.raw_exec_fee_factor).to_signed_bytes_le())
            }
            "getExecFeeFactor" => Ok(BigInt::from(self.raw_exec_fee_factor).to_signed_bytes_le()),
            "getStoragePrice" => Ok(BigInt::from(100_000).to_signed_bytes_le()),
            other => Err(CoreError::invalid_operation(format!(
                "unexpected PolicyContract method {other}"
            ))),
        }
    }
}

struct TestPolicyProvider {
    policy: Arc<TestPolicyContract>,
}

impl NativeContractProvider for TestPolicyProvider {
    type Contract = crate::native_contract_provider::NoNativeContract;

    fn exec_fee_factor_raw<B: CacheRead>(&self, _snapshot: &DataCache<B>) -> CoreResult<u32> {
        u32::try_from(self.policy.raw_exec_fee_factor)
            .map_err(|_| CoreError::invalid_operation("invalid raw execution fee factor"))
    }

    fn storage_price<B: CacheRead>(&self, _snapshot: &DataCache<B>) -> CoreResult<u32> {
        Ok(100_000)
    }
}

fn with_test_policy(raw_exec_fee_factor: i64, test: impl FnOnce(Arc<TestPolicyProvider>)) {
    let provider = Arc::new(TestPolicyProvider {
        policy: Arc::new(TestPolicyContract {
            raw_exec_fee_factor,
        }),
    });
    test(provider);
}

#[test]
fn notification_parameter_type_check_matches_csharp() {
    assert!(!matches_parameter_type(
        &StackItem::null(),
        ContractParameterType::String
    ));
    assert!(matches_parameter_type(
        &StackItem::from_byte_string(b"neo".to_vec()),
        ContractParameterType::String
    ));
    assert!(!matches_parameter_type(
        &StackItem::from_byte_string(vec![0xff]),
        ContractParameterType::String
    ));

    let pointer = StackItem::from_pointer(Arc::new(Script::new_from_bytes(vec![])), 0);
    for expected in [
        ContractParameterType::Any,
        ContractParameterType::ByteArray,
        ContractParameterType::InteropInterface,
    ] {
        assert!(
            !matches_parameter_type(&pointer, expected),
            "C# CheckItemType rejects Pointer before matching {expected:?}"
        );
    }
}

#[test]
fn runtime_log_allows_dynamic_script_without_container_like_csharp() {
    let mut builder = ScriptBuilder::new();
    builder.emit_push_string("dynamic log");
    builder
        .emit_syscall("System.Runtime.Log")
        .expect("emit Runtime.Log");
    builder.emit_opcode(OpCode::RET);

    let mut engine =
        ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            NoDiagnostic,
            Arc::new(NoNativeContractProvider),
        )
        .expect("application engine");
    engine
        .load_script(builder.to_array(), CallFlags::ALLOW_NOTIFY, None)
        .expect("load script");

    assert_eq!(engine.execute_allow_fault(), VMState::HALT);

    let log = engine.logs().first().expect("log event");
    assert!(log.script_container.is_none());
    assert_eq!(log.message, "dynamic log");
}

#[test]
fn send_notification_enforces_echidna_cap_for_native_paths_like_csharp() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.clear();
    settings.hardforks.insert(Hardfork::HfEchidna, 0);

    let mut engine =
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
        .expect("application engine");
    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALLOW_NOTIFY, None)
        .expect("load script");

    for _ in 0..crate::application_engine::MAX_NOTIFICATION_COUNT {
        engine
            .send_notification(UInt160::zero(), "Native".to_string(), Vec::new())
            .expect("notification below cap");
    }

    let err = engine
        .send_notification(UInt160::zero(), "Native".to_string(), Vec::new())
        .expect_err("513th application notification must fault after Echidna");
    assert!(err.to_string().contains("Maximum number of notifications"));
    assert_eq!(
        engine.notifications().len(),
        crate::application_engine::MAX_NOTIFICATION_COUNT
    );
}

#[test]
fn get_notifications_deep_copies_domovoi_state_like_csharp() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.clear();
    settings.hardforks.insert(Hardfork::HfDomovoi, 0);

    let mut engine =
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
        .expect("application engine");
    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALLOW_NOTIFY, None)
        .expect("load script");

    let nested_arg = StackItem::from_array(vec![StackItem::from_i64(1)]);
    engine
        .send_notification(
            UInt160::zero(),
            "Native".to_string(),
            vec![nested_arg.clone(), nested_arg],
        )
        .expect("send notification");

    let StackItem::Array(stored_nested) = &engine.notifications()[0].state()[0] else {
        panic!("stored notification argument should be an array");
    };
    let StackItem::Array(stored_nested_alias) = &engine.notifications()[0].state()[1] else {
        panic!("stored notification alias should be an array");
    };
    assert_eq!(stored_nested.id(), stored_nested_alias.id());
    assert!(stored_nested.is_read_only());
    assert!(stored_nested.push(StackItem::from_i64(2)).is_err());

    let notifications = engine.get_notifications(None).expect("get notifications");
    let StackItem::Array(notification) = &notifications[0] else {
        panic!("notification should project as an array");
    };
    let fields = notification.items();
    assert_eq!(fields.len(), 3);

    let StackItem::Array(state) = &fields[2] else {
        panic!("notification state should project as an array");
    };
    assert!(state.is_read_only());

    let state_items = state.items();
    let StackItem::Array(returned_nested) = &state_items[0] else {
        panic!("returned notification argument should be an array");
    };
    let StackItem::Array(returned_nested_alias) = &state_items[1] else {
        panic!("returned notification alias should be an array");
    };
    assert!(returned_nested.is_read_only());
    assert_eq!(returned_nested.id(), returned_nested_alias.id());
    assert_ne!(returned_nested.id(), stored_nested.id());
    assert!(returned_nested.push(StackItem::from_i64(3)).is_err());

    let second_notifications = engine
        .get_notifications(None)
        .expect("get notifications again");
    let StackItem::Array(second_notification) = &second_notifications[0] else {
        panic!("second notification should project as an array");
    };
    let second_fields = second_notification.items();
    let StackItem::Array(second_state) = &second_fields[2] else {
        panic!("second notification state should project as an array");
    };
    let second_state_items = second_state.items();
    let StackItem::Array(second_returned_nested) = &second_state_items[0] else {
        panic!("second returned argument should be an array");
    };
    assert_ne!(state.id(), second_state.id());
    assert_ne!(returned_nested.id(), second_returned_nested.id());
}

#[test]
fn notification_size_check_uses_stack_value_serializer() {
    let source = include_str!("../../interop/application_engine_helper.rs");
    let start = source
        .find("pub fn ensure_notification_size")
        .expect("ensure_notification_size exists");
    let end = source[start..]
        .find("pub fn send_notification")
        .map(|offset| start + offset)
        .expect("send_notification follows size check");
    let helper = &source[start..end];

    assert!(helper.contains("notification_state_to_stack_value"));
    assert!(helper.contains("serialize_stack_value_with_limits"));
    assert!(!helper.contains("StackItem::from_array(state.to_vec())"));
    assert!(!helper.contains("BinarySerializer::serialize(&StackItem"));
}

#[test]
fn get_notifications_reuses_pre_domovoi_immutable_state_like_csharp() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.clear();

    let mut engine =
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
        .expect("application engine");
    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALLOW_NOTIFY, None)
        .expect("load script");

    let nested = StackItem::from_array(vec![StackItem::from_i64(1)]);
    engine
        .send_notification(
            UInt160::zero(),
            "Native".to_string(),
            vec![nested.clone(), nested],
        )
        .expect("send notification");

    let stored_state = engine.notifications()[0].state_array();
    let StackItem::Array(stored_state) = stored_state else {
        panic!("stored state should be an array");
    };
    let stored_items = stored_state.items();
    let StackItem::Array(stored_nested) = &stored_items[0] else {
        panic!("stored nested state should be an array");
    };
    let StackItem::Array(stored_nested_alias) = &stored_items[1] else {
        panic!("stored nested alias should be an array");
    };
    assert!(stored_state.is_read_only());
    assert!(stored_nested.is_read_only());
    assert_eq!(stored_nested.id(), stored_nested_alias.id());

    let first = engine.get_notifications(None).expect("get notifications");
    let second = engine
        .get_notifications(None)
        .expect("get notifications again");
    let StackItem::Array(first_notification) = &first[0] else {
        panic!("first notification should be an array");
    };
    let StackItem::Array(second_notification) = &second[0] else {
        panic!("second notification should be an array");
    };
    let first_fields = first_notification.items();
    let second_fields = second_notification.items();
    let StackItem::Array(first_state) = &first_fields[2] else {
        panic!("first state should be an array");
    };
    let StackItem::Array(second_state) = &second_fields[2] else {
        panic!("second state should be an array");
    };
    assert_eq!(first_state.id(), stored_state.id());
    assert_eq!(second_state.id(), stored_state.id());
    assert!(first_state.push(StackItem::from_i64(2)).is_err());

    let first_state_items = first_state.items();
    let StackItem::Array(first_nested) = &first_state_items[0] else {
        panic!("first nested state should be an array");
    };
    let StackItem::Array(first_nested_alias) = &first_state_items[1] else {
        panic!("first nested alias should be an array");
    };
    assert_eq!(first_nested.id(), stored_nested.id());
    assert_eq!(first_nested.id(), first_nested_alias.id());
    assert!(first_nested.push(StackItem::from_i64(3)).is_err());
}

#[test]
fn runtime_check_witness_faults_on_invalid_public_key_like_csharp() {
    let invalid_public_key = [0x05; 33];
    let mut builder = ScriptBuilder::new();
    builder.emit_push(&invalid_public_key);
    builder
        .emit_syscall("System.Runtime.CheckWitness")
        .expect("emit Runtime.CheckWitness");
    builder.emit_opcode(OpCode::RET);

    let mut engine =
        ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            NoDiagnostic,
            Arc::new(NoNativeContractProvider),
        )
        .expect("application engine");
    engine
        .load_script(builder.to_array(), CallFlags::NONE, None)
        .expect("load script");

    assert_eq!(engine.execute_allow_fault(), VMState::FAULT);
}

#[test]
fn verification_trigger_without_persisting_block_uses_configured_hardforks_like_csharp() {
    let settings = ProtocolSettings::default();
    assert!(settings.hardforks.contains_key(&Hardfork::HfAspidochelone));

    let mut builder = ScriptBuilder::new();
    builder
        .emit_syscall("System.Runtime.GetRandom")
        .expect("emit Runtime.GetRandom");
    builder.emit_opcode(OpCode::RET);

    let mut engine =
        ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
            TriggerType::Verification,
            None,
            Arc::new(DataCache::new(false)),
            None,
            settings,
            TEST_MODE_GAS,
            NoDiagnostic,
            Arc::new(NoNativeContractProvider),
        )
        .expect("application engine");
    engine
        .load_script(builder.to_array(), CallFlags::NONE, None)
        .expect("load script");

    assert_eq!(engine.execute_allow_fault(), VMState::HALT);
    assert_eq!(engine.fee_consumed(), (1 << 13) * 30);
}

#[test]
fn default_policy_storage_exec_fee_factor_charges_push1_as_thirty_datoshi() {
    let snapshot = Arc::new(DataCache::new(false));
    seed_policy_exec_fee_factor(&snapshot, 30);

    let mut engine =
        ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
            TriggerType::Application,
            None,
            Arc::clone(&snapshot),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            NoDiagnostic,
            Arc::new(NoNativeContractProvider),
        )
        .expect("application engine");
    engine
        .load_script(vec![OpCode::PUSH1.byte()], CallFlags::ALL, None)
        .expect("load script");

    assert_eq!(engine.execute_allow_fault(), VMState::HALT);
    assert_eq!(
        engine.fee_consumed(),
        30,
        "genesis-style Policy ExecFeeFactor storage remains datoshi-scaled until HF_Faun migration rewrites it to pico-GAS"
    );
}

#[test]
fn policy_provider_legacy_exec_fee_factor_is_scaled_until_faun_height() {
    with_test_policy(30, |provider| {
        let snapshot = Arc::new(DataCache::new(false));
        seed_policy_exec_fee_factor(&snapshot, 30);

        let mut engine = ApplicationEngine::new_with_native_contract_provider(
            TriggerType::Application,
            None,
            Arc::clone(&snapshot),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            NoDiagnostic,
            provider,
        )
        .expect("application engine");
        engine
            .load_script(vec![OpCode::PUSH1.byte()], CallFlags::ALL, None)
            .expect("load script");

        assert_eq!(engine.execute_allow_fault(), VMState::HALT);
        assert_eq!(engine.fee_consumed(), 30);
    });
}

#[test]
fn application_engine_loop_hits_gas_limit_before_instruction_cap() {
    let mut builder = ScriptBuilder::new();
    builder.emit_jump(OpCode::JMP_L, 0).expect("jump loop");

    let mut engine =
        ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            1_000_000,
            NoDiagnostic,
            Arc::new(NoNativeContractProvider),
        )
        .expect("application engine");
    engine
        .load_script(builder.to_array(), CallFlags::ALL, None)
        .expect("load script");

    assert_eq!(engine.execute_allow_fault(), VMState::FAULT);
    let exception = engine.fault_exception().unwrap_or_default();
    assert!(
        exception.to_ascii_lowercase().contains("gas exhausted"),
        "expected local-engine gas exhaustion before VM instruction cap, got {exception}"
    );
    assert!(engine.fee_consumed() >= 1_000_000);
}

#[test]
fn invocation_counter_uses_explicit_context_script_hash_like_csharp() {
    let logical_hash = UInt160::from_bytes(&[0x42; 20]).expect("logical hash");

    let mut builder = ScriptBuilder::new();
    builder
        .emit_syscall("System.Runtime.GetInvocationCounter")
        .expect("emit Runtime.GetInvocationCounter");
    builder.emit_opcode(OpCode::RET);

    let mut engine =
        ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            NoDiagnostic,
            Arc::new(NoNativeContractProvider),
        )
        .expect("application engine");
    engine
        .load_script(builder.to_array(), CallFlags::NONE, Some(logical_hash))
        .expect("load script with logical hash");

    assert_eq!(engine.execute_allow_fault(), VMState::HALT);

    let result = engine
        .result_stack()
        .peek(0)
        .expect("invocation counter result")
        .as_int()
        .expect("integer result");
    assert_eq!(result, 1.into());
}

#[test]
fn pointer_result_halts_like_neo_vm_v3101() {
    // A pure `PUSHA <offset>; RET` script HALTs with a Pointer on the result
    // stack in NeoVM v3.10.1. Canonical execution must preserve that result.
    let script = vec![
        OpCode::PUSHA.byte(),
        0x00,
        0x00,
        0x00,
        0x00,
        OpCode::RET.byte(),
    ];
    let mut engine =
        ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            NoDiagnostic,
            Arc::new(NoNativeContractProvider),
        )
        .expect("application engine");
    engine
        .load_script(script, CallFlags::NONE, None)
        .expect("load script");

    assert_eq!(engine.execute_allow_fault(), VMState::HALT);
    assert!(matches!(
        engine.result_stack().peek(0),
        Ok(neo_vm::stack_item::StackItem::Pointer(_))
    ));
}

#[test]
fn canonical_execution_does_not_dispatch_to_external_interpreter() {
    let source = include_str!("../../application_engine/storage_ops/load_execute_storage.rs");
    let start = source
        .find("pub fn execute_allow_fault")
        .expect("execute_allow_fault exists");
    let end = source[start..]
        .find("pub fn execute_until_invocation_stack_depth")
        .map(|offset| start + offset)
        .expect("execute_until_invocation_stack_depth follows execute_allow_fault");
    let execute_allow_fault = &source[start..end];

    assert!(execute_allow_fault.contains("self.vm_engine.engine_mut().execute()"));
    assert!(!execute_allow_fault.contains("try_execute_with_external_vm"));
}

#[test]
fn zero_shift_coerces_boolean_result_to_integer_like_neo_vm_v3101() {
    // NeoVM v3.10.1 unconditionally integer-coerces SHL's value operand,
    // including when the shift is zero. Older/external interpreters have
    // preserved the Boolean here, which changes the consensus stack type.
    let script = vec![
        OpCode::PUSHT.byte(),
        OpCode::PUSH0.byte(),
        OpCode::SHL.byte(),
        OpCode::RET.byte(),
    ];
    let mut engine =
        ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            NoDiagnostic,
            Arc::new(NoNativeContractProvider),
        )
        .expect("application engine");
    engine
        .load_script(script, CallFlags::NONE, None)
        .expect("load script");

    assert_eq!(engine.execute_allow_fault(), VMState::HALT);
    let result = engine.result_stack().peek(0).expect("shift result");
    assert!(
        matches!(result, neo_vm::stack_item::StackItem::Integer(_)),
        "zero-shift result must be an Integer, got {result:?}"
    );
    assert_eq!(result.as_int().expect("integer result"), BigInt::from(1));
}
