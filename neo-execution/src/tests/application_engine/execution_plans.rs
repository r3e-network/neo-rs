use super::*;
use crate::native_contract_provider::NoNativeContractProvider;
use neo_manifest::{ContractAbi, ContractManifest, NefFile};
use neo_primitives::ContractBasicMethod;
use neo_vm::OpCode;

fn engine_with_settings(settings: ProtocolSettings) -> ApplicationEngine {
    ApplicationEngine::<NoNativeContractProvider>::new_with_shared_block_and_native_contract_provider(
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

fn enabled_config() -> ApplicationExecutionPlanConfig {
    ApplicationExecutionPlanConfig::Enabled {
        cache_limits: ExecutionPlanCacheLimits {
            max_entries: 16,
            max_bytes: 1024 * 1024,
        },
        plan_limits: ExecutionPlanLimits::DEFAULT,
    }
}

fn test_contract(script: Vec<u8>) -> ContractState {
    ContractState::new(
        17,
        UInt160::from_bytes(&[0x42; 20]).expect("contract hash"),
        NefFile::new("execution-plan-test".to_string(), script),
        ContractManifest::new("ExecutionPlanTest".to_string()),
    )
}

#[test]
fn execution_plans_are_disabled_by_default_and_use_the_ordinary_route() {
    let mut engine = engine_with_settings(ProtocolSettings::mainnet());

    assert_eq!(
        engine.execution_plan_config(),
        ApplicationExecutionPlanConfig::Disabled
    );
    assert!(engine.execution_plan_cache_snapshot().is_none());

    engine
        .load_script(
            vec![OpCode::PUSH1.byte(), OpCode::RET.byte()],
            CallFlags::ALL,
            None,
        )
        .expect("load ordinary script");
    assert!(
        engine
            .vm_engine
            .engine()
            .current_context()
            .expect("ordinary context")
            .execution_plan()
            .is_none()
    );
    assert_eq!(engine.execute_allow_fault(), VMState::HALT);
}

#[test]
fn enabled_route_attaches_the_exact_v3101_plan_before_context_publication() {
    let mut engine = engine_with_settings(ProtocolSettings::mainnet());
    engine.set_execution_plan_config(enabled_config());

    let script = vec![OpCode::NOP.byte(), OpCode::PUSH1.byte(), OpCode::RET.byte()];
    engine
        .load_script(script.clone(), CallFlags::ALL, None)
        .expect("load planned script");

    let context = engine
        .vm_engine
        .engine()
        .current_context()
        .expect("planned context");
    let plan = context.execution_plan().expect("exact plan attached");
    let key = plan.key();
    assert_eq!(key.script_bytes(), script);
    assert_eq!(key.entry_ip(), 0);
    assert_eq!(key.trigger(), TriggerType::Application);
    assert_eq!(
        key.protocol().network_magic(),
        engine.protocol_settings.network
    );
    assert_eq!(key.protocol().version(), ProtocolVersion::NEO_N3_V3_10_1);
    assert_eq!(key.contract(), None);
    assert_eq!(
        key.hardforks().state(Hardfork::HfAspidochelone),
        HardforkPlanState::Pending {
            activation_height: 1_730_000,
        }
    );
    assert_eq!(
        key.hardforks().state(Hardfork::HfHuyao),
        HardforkPlanState::Unconfigured
    );

    let snapshot = engine
        .execution_plan_cache_snapshot()
        .expect("enabled cache snapshot");
    assert_eq!(snapshot.ready_entries, 1);
    assert_eq!(snapshot.builds, 1);
    assert_eq!(engine.execute_allow_fault(), VMState::HALT);
}

#[test]
fn deployed_contract_route_includes_the_exact_resolution_identity() {
    let mut engine = engine_with_settings(ProtocolSettings::mainnet());
    engine.set_execution_plan_config(enabled_config());
    let mut contract = test_contract(vec![OpCode::PUSH1.byte(), OpCode::RET.byte()]);
    contract.update_counter = 3;
    let method = ContractMethodDescriptor::new(
        "value".to_string(),
        Vec::new(),
        ContractParameterType::Integer,
        0,
        true,
    )
    .expect("contract method");

    engine
        .load_contract_method(contract.clone(), method, CallFlags::ALL)
        .expect("load planned contract method");
    let plan = engine
        .vm_engine
        .engine()
        .current_context()
        .expect("contract context")
        .execution_plan()
        .expect("contract plan");
    let identity = plan.key().contract().expect("contract resolution identity");
    assert_eq!(identity.contract_hash(), contract.hash);
    assert_eq!(identity.contract_id(), contract.id);
    assert_eq!(identity.update_counter(), contract.update_counter);
    assert_eq!(identity.nef_checksum(), contract.nef.checksum);
    assert_eq!(plan.key().entry_ip(), 0);
    assert_eq!(engine.execute_allow_fault(), VMState::HALT);
}

#[test]
fn contract_initializer_clone_shares_the_host_entry_plan() {
    let mut engine = engine_with_settings(ProtocolSettings::mainnet());
    engine.set_execution_plan_config(enabled_config());
    let initialize = ContractMethodDescriptor::new(
        ContractBasicMethod::INITIALIZE.to_string(),
        Vec::new(),
        ContractParameterType::Void,
        1,
        false,
    )
    .expect("initialize method");
    let method = ContractMethodDescriptor::new(
        "run".to_string(),
        Vec::new(),
        ContractParameterType::Void,
        0,
        false,
    )
    .expect("run method");
    let mut contract = test_contract(vec![OpCode::RET.byte(), OpCode::RET.byte()]);
    contract.manifest.abi = ContractAbi::new(vec![method.clone(), initialize], Vec::new());

    engine
        .load_contract_method(contract, method, CallFlags::ALL)
        .expect("load contract with initializer");
    let contexts = engine.vm_engine.engine().invocation_stack();
    assert_eq!(contexts.len(), 2);
    let method_plan = contexts[0].execution_plan().expect("method plan");
    let initialize_plan = contexts[1].execution_plan().expect("initializer plan");
    assert!(Arc::ptr_eq(method_plan, initialize_plan));
    assert_eq!(method_plan.key().entry_ip(), 0);
    assert_eq!(contexts[0].instruction_pointer(), 0);
    assert_eq!(contexts[1].instruction_pointer(), 1);
    let snapshot = engine
        .execution_plan_cache_snapshot()
        .expect("enabled cache snapshot");
    assert_eq!(snapshot.builds, 1);
    assert_eq!(snapshot.ready_entries, 1);
    assert_eq!(engine.execute_allow_fault(), VMState::HALT);
}

#[test]
fn entry_contract_update_and_hardfork_identity_select_distinct_plans() {
    let mut settings = ProtocolSettings::mainnet();
    settings.hardforks = settings
        .hardforks
        .with_activation(Hardfork::HfAspidochelone, 10);
    let mut engine = engine_with_settings(settings);
    engine.set_execution_plan_config(enabled_config());

    let contract = test_contract(vec![OpCode::NOP.byte(), OpCode::RET.byte()]);
    let script = Script::new_relaxed(contract.nef.script.clone());
    let at_zero = engine.prepare_script_with_execution_plan(script.clone(), 0, Some(&contract));
    let key_zero = at_zero
        .execution_plan()
        .expect("entry-zero plan")
        .key()
        .clone();

    let at_one = engine.prepare_script_with_execution_plan(script.clone(), 1, Some(&contract));
    let key_one = at_one
        .execution_plan()
        .expect("entry-one plan")
        .key()
        .clone();
    assert_ne!(key_zero, key_one);
    assert_eq!(key_one.entry_ip(), 1);

    let mut updated_contract = contract.clone();
    updated_contract.update_counter = 1;
    let updated =
        engine.prepare_script_with_execution_plan(script.clone(), 0, Some(&updated_contract));
    let updated_key = updated
        .execution_plan()
        .expect("updated-contract plan")
        .key()
        .clone();
    assert_ne!(key_zero, updated_key);
    assert_eq!(
        updated_key
            .contract()
            .expect("contract resolution")
            .update_counter(),
        1
    );

    let settings = Arc::make_mut(&mut engine.protocol_settings);
    settings.hardforks = settings
        .hardforks
        .with_activation(Hardfork::HfAspidochelone, 0);
    let after_hardfork =
        engine.prepare_script_with_execution_plan(script, 0, Some(&updated_contract));
    let hardfork_key = after_hardfork
        .execution_plan()
        .expect("post-hardfork plan")
        .key();
    assert_ne!(&updated_key, hardfork_key);
    assert_eq!(
        updated_key.hardforks().state(Hardfork::HfAspidochelone),
        HardforkPlanState::Pending {
            activation_height: 10,
        }
    );
    assert_eq!(
        hardfork_key.hardforks().state(Hardfork::HfAspidochelone),
        HardforkPlanState::Active {
            activation_height: 0,
        }
    );

    let snapshot = engine
        .execution_plan_cache_snapshot()
        .expect("enabled cache snapshot");
    assert_eq!(snapshot.ready_entries, 4);
    assert_eq!(snapshot.builds, 4);
}

#[test]
fn cache_and_build_failures_fall_back_before_context_publication() {
    let mut capacity_engine = engine_with_settings(ProtocolSettings::mainnet());
    capacity_engine.set_execution_plan_config(ApplicationExecutionPlanConfig::Enabled {
        cache_limits: ExecutionPlanCacheLimits {
            max_entries: 0,
            max_bytes: 0,
        },
        plan_limits: ExecutionPlanLimits::DEFAULT,
    });
    capacity_engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("capacity fallback loads ordinary context");
    assert!(
        capacity_engine
            .vm_engine
            .engine()
            .current_context()
            .expect("capacity fallback context")
            .execution_plan()
            .is_none()
    );
    assert_eq!(capacity_engine.execute_allow_fault(), VMState::HALT);
    assert_eq!(
        capacity_engine
            .execution_plan_cache_snapshot()
            .expect("capacity cache")
            .capacity_rejections,
        1
    );

    // The relaxed ordinary VM can return before decoding unreachable trailing
    // garbage, while strict whole-script plan construction rejects it.
    let mut build_engine = engine_with_settings(ProtocolSettings::mainnet());
    build_engine.set_execution_plan_config(enabled_config());
    build_engine
        .load_script(vec![OpCode::RET.byte(), 0xff], CallFlags::ALL, None)
        .expect("build failure loads ordinary context");
    assert!(
        build_engine
            .vm_engine
            .engine()
            .current_context()
            .expect("build fallback context")
            .execution_plan()
            .is_none()
    );
    assert_eq!(build_engine.execute_allow_fault(), VMState::HALT);
    assert_eq!(
        build_engine
            .execution_plan_cache_snapshot()
            .expect("build cache")
            .build_failures,
        1
    );
}

#[test]
fn exact_byte_or_entry_mismatch_keeps_the_original_script_unplanned() {
    let mut engine = engine_with_settings(ProtocolSettings::mainnet());
    engine.set_execution_plan_config(enabled_config());
    let planned = engine.prepare_script_with_execution_plan(
        Script::new_relaxed(vec![OpCode::NOP.byte(), OpCode::RET.byte()]),
        0,
        None,
    );
    let plan = Arc::clone(planned.execution_plan().expect("source plan"));

    let byte_mismatch =
        ApplicationEngine::<NoNativeContractProvider>::attach_execution_plan_or_fallback(
            Script::new_relaxed(vec![OpCode::PUSH1.byte(), OpCode::RET.byte()]),
            0,
            Ok(Arc::clone(&plan)),
        );
    assert!(byte_mismatch.execution_plan().is_none());

    let entry_mismatch =
        ApplicationEngine::<NoNativeContractProvider>::attach_execution_plan_or_fallback(
            Script::new_relaxed(vec![OpCode::NOP.byte(), OpCode::RET.byte()]),
            1,
            Ok(plan),
        );
    assert!(entry_mismatch.execution_plan().is_none());
}

#[test]
fn prepare_next_transaction_retains_the_bounded_plan_cache() {
    let mut engine = engine_with_settings(ProtocolSettings::mainnet());
    engine.set_execution_plan_config(enabled_config());
    let script = vec![OpCode::NOP.byte(), OpCode::RET.byte()];
    engine
        .load_script(script.clone(), CallFlags::ALL, None)
        .expect("first planned load");
    assert_eq!(
        engine
            .execution_plan_cache_snapshot()
            .expect("first cache snapshot")
            .builds,
        1
    );

    engine.prepare_next_transaction(None, Arc::new(DataCache::new(false)), TEST_MODE_GAS);
    engine
        .load_script(script, CallFlags::ALL, None)
        .expect("second planned load");
    let snapshot = engine
        .execution_plan_cache_snapshot()
        .expect("second cache snapshot");
    assert_eq!(snapshot.builds, 1);
    assert_eq!(snapshot.hits, 1);
    assert_eq!(snapshot.ready_entries, 1);
}

#[test]
fn explicit_shared_cache_reuses_warm_plans_across_engine_instances() {
    let script = vec![OpCode::NOP.byte(), OpCode::RET.byte()];
    let mut first = engine_with_settings(ProtocolSettings::mainnet());
    first.set_execution_plan_config(enabled_config());
    first
        .load_script(script.clone(), CallFlags::ALL, None)
        .expect("first planned load");
    let shared = first
        .shared_execution_plan_cache()
        .expect("explicit shared cache");
    assert_eq!(shared.config(), enabled_config());

    let mut second = engine_with_settings(ProtocolSettings::mainnet());
    assert_eq!(
        second.execution_plan_config(),
        ApplicationExecutionPlanConfig::Disabled
    );
    second.set_shared_execution_plan_cache(shared.clone());
    second
        .load_script(script, CallFlags::ALL, None)
        .expect("warm planned load");

    let snapshot = shared.snapshot();
    assert_eq!(snapshot.builds, 1);
    assert_eq!(snapshot.hits, 1);
    assert_eq!(snapshot.ready_entries, 1);
    assert_eq!(
        second.execution_plan_config(),
        first.execution_plan_config()
    );
}
