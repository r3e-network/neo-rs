use super::*;
use crate::NativeContract;
use crate::application_engine::TEST_MODE_GAS;
use crate::native_contract_provider::{NativeContractLookup, NativeContractProvider};
use neo_config::ProtocolSettings;
use neo_primitives::TriggerType;
use neo_storage::DataCache;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::Script;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::OpCode;
use neo_vm_rs::VmState as VMState;
use num_bigint::BigInt;
use std::sync::Arc;

const POLICY_CONTRACT_ID: i32 = -7;
const POLICY_PREFIX_EXEC_FEE_FACTOR: u8 = 18;

fn seed_policy_exec_fee_factor(snapshot: &DataCache, value: i64) {
    snapshot.add(
        StorageKey::new(POLICY_CONTRACT_ID, vec![POLICY_PREFIX_EXEC_FEE_FACTOR]),
        StorageItem::from_bytes(BigInt::from(value).to_signed_bytes_le()),
    );
}

struct TestPolicyContract {
    raw_exec_fee_factor: i64,
}

impl NativeContract for TestPolicyContract {
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

    fn invoke(
        &self,
        _engine: &mut ApplicationEngine,
        method: &str,
        _args: &[Vec<u8>],
    ) -> CoreResult<Vec<u8>> {
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

struct TestPolicyProvider {
    policy: Arc<TestPolicyContract>,
}

impl NativeContractProvider for TestPolicyProvider {
    fn get_native_contract(&self, hash: &UInt160) -> Option<Arc<dyn NativeContract>> {
        (&self.policy.hash() == hash).then(|| self.policy.clone() as Arc<dyn NativeContract>)
    }

    fn get_native_contract_by_name(&self, name: &str) -> Option<Arc<dyn NativeContract>> {
        name.eq_ignore_ascii_case("PolicyContract")
            .then(|| self.policy.clone() as Arc<dyn NativeContract>)
    }

    fn all_native_contracts(&self) -> Vec<Arc<dyn NativeContract>> {
        vec![self.policy.clone() as Arc<dyn NativeContract>]
    }

    fn all_native_contract_hashes(&self) -> Vec<UInt160> {
        vec![self.policy.hash()]
    }
}

fn with_test_policy(raw_exec_fee_factor: i64, test: impl FnOnce()) {
    NativeContractLookup::with_scoped_provider(
        Arc::new(TestPolicyProvider {
            policy: Arc::new(TestPolicyContract {
                raw_exec_fee_factor,
            }),
        }),
        test,
    );
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

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
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

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        settings,
        TEST_MODE_GAS,
        None,
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

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        settings,
        TEST_MODE_GAS,
        None,
    )
    .expect("application engine");
    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALLOW_NOTIFY, None)
        .expect("load script");

    let nested_arg = StackItem::from_array(vec![StackItem::from_i64(1)]);
    engine
        .send_notification(UInt160::zero(), "Native".to_string(), vec![nested_arg])
        .expect("send notification");

    let StackItem::Array(stored_nested) = &engine.notifications()[0].state[0] else {
        panic!("stored notification argument should be an array");
    };
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
    assert!(returned_nested.is_read_only());
    assert_ne!(returned_nested.id(), stored_nested.id());
    assert!(returned_nested.push(StackItem::from_i64(3)).is_err());
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
fn get_notifications_non_domovoi_projection_uses_stack_value_adapter() {
    let source = include_str!("../../interop/application_engine_helper.rs");
    let start = source
        .find("fn notification_to_stack_item")
        .expect("notification projection helper exists");
    let end = source[start..]
        .find("fn notification_state_to_stack_value")
        .map(|offset| start + offset)
        .expect("stack value helper follows notification projection");
    let helper = &source[start..end];

    assert!(helper.contains("readonly_array_stack_item"));
    assert!(helper.contains("notification_state_to_stack_value(&notification.state)"));
    assert!(helper.contains("StackItem::try_from(value)"));
    assert!(!helper.contains("StackItem::from_array(notification.state.to_vec())"));
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

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
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

    let mut engine = ApplicationEngine::new(
        TriggerType::Verification,
        None,
        Arc::new(DataCache::new(false)),
        None,
        settings,
        TEST_MODE_GAS,
        None,
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

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::clone(&snapshot),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
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
    with_test_policy(30, || {
        let snapshot = Arc::new(DataCache::new(false));
        seed_policy_exec_fee_factor(&snapshot, 30);

        let mut engine = ApplicationEngine::new(
            TriggerType::Application,
            None,
            Arc::clone(&snapshot),
            None,
            ProtocolSettings::default(),
            TEST_MODE_GAS,
            None,
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
fn external_vm_loop_hits_gas_limit_before_instruction_cap() {
    let mut builder = ScriptBuilder::new();
    builder.emit_jump(OpCode::JMP_L, 0).expect("jump loop");

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        1_000_000,
        None,
    )
    .expect("application engine");
    engine
        .load_script(builder.to_array(), CallFlags::ALL, None)
        .expect("load script");

    assert_eq!(engine.execute_allow_fault(), VMState::FAULT);
    let exception = engine.fault_exception().unwrap_or_default();
    assert!(
        exception.to_ascii_lowercase().contains("insufficient gas"),
        "expected insufficient gas before VM instruction cap, got {exception}"
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

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
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
fn external_vm_pointer_result_halts_like_local_engine() {
    // A pure `PUSHA <offset>; RET` script HALTs with a Pointer on the result
    // stack. The neo-vm-rs fast path cannot represent a Pointer as a stateful
    // StackItem, so it must DECLINE and let the local engine HALT with the
    // Pointer (matching C#/the local engine) rather than FAULT — otherwise a
    // crafted transaction would HALT on C# but FAULT on the fast path, which is
    // a consensus divergence.
    let script = vec![
        OpCode::PUSHA.byte(),
        0x00,
        0x00,
        0x00,
        0x00,
        OpCode::RET.byte(),
    ];
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
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
