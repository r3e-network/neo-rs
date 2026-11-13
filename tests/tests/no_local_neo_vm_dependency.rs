use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use neo_primitives::CallFlags;
use neo_primitives::TriggerType;
use neo_primitives::{UInt160, WitnessScope};
use neo_protocol::ProtocolSettings;
use neo_protocol::payloads::signer::Signer;
use neo_protocol::payloads::transaction::Transaction;
use neo_smart_contract::application_engine::{ApplicationEngine, TEST_MODE_GAS};
use neo_storage::persistence::DataCache;
use neo_vm::{
    ExceptionHandlingContext, ExceptionHandlingState, ExecutionEngine, ExecutionEngineLimits,
    OpCode, Script, ScriptBuilder, StackItem, StackValue, VmState as VMState,
};
use num_bigint::BigInt;

#[test]
fn workspace_vendors_neo_vm_as_the_canonical_local_crate() {
    let workspace = workspace_root();
    let root_manifest = fs::read_to_string(workspace.join("Cargo.toml")).unwrap();

    // The final architecture vendors the VM as a self-contained local crate;
    // it is a workspace member and the shared `neo-vm` workspace dependency.
    assert!(
        root_manifest.contains("\"neo-vm\""),
        "root workspace must keep the local neo-vm crate as a member"
    );
    assert!(
        root_manifest.contains("neo-vm = { path = \"neo-vm\""),
        "root workspace dependencies must expose the local neo-vm crate"
    );

    for manifest in cargo_manifests(&workspace) {
        // The neo-vm crate's own manifest legitimately declares its name.
        if manifest
            .parent()
            .is_some_and(|dir| dir == workspace.join("neo-vm"))
        {
            continue;
        }
        let manifest_text = fs::read_to_string(&manifest).unwrap();
        assert!(
            !manifest_text.contains("name = \"neo-vm\"")
                && !manifest_text.contains("name = 'neo-vm'"),
            "{} must not re-declare a second local neo-vm crate",
            manifest.display()
        );
    }
}

#[test]
fn application_engine_executes_syscall_free_scripts_through_neo_vm_rs() {
    let workspace = workspace_root();
    let load_execute = read_source(
        workspace.join("neo-smart-contract/src/application_engine/load_execute_storage.rs"),
    );
    let external_vm =
        read_source(workspace.join("neo-smart-contract/src/application_engine/external_vm.rs"));

    assert!(
        load_execute.contains("try_execute_with_external_vm()"),
        "ApplicationEngine::execute_allow_fault should try the neo-vm-rs execution boundary"
    );
    assert!(
        external_vm.contains("interpret_with_stack_and_syscalls_at")
            && external_vm.contains("impl SyscallProvider for ExternalVmHost"),
        "the external execution boundary should use neo-vm-rs interpreter APIs directly"
    );
    assert!(
        external_vm.contains("parse_script_instructions")
            && !external_vm.contains("let mut position = 0"),
        "the external execution boundary should use neo-vm-rs script parsing for \
         eligibility scans instead of local instruction-walking loops"
    );

    let mut builder = ScriptBuilder::new();
    builder.emit_push_int(2);
    builder.emit_push_int(5);
    builder.emit_opcode(OpCode::ADD);
    builder.emit_opcode(OpCode::RET);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
        neo_native_contracts::native::composition::default_native_services(),
    )
    .expect("application engine");
    engine
        .load_script(builder.to_array(), CallFlags::NONE, None)
        .expect("load script");

    assert_eq!(engine.execute_allow_fault(), VMState::HALT);
    assert_eq!(
        engine.result_stack().peek(0).unwrap().as_int().unwrap(),
        BigInt::from(7)
    );
}

#[test]
fn application_engine_routes_simple_runtime_syscalls_through_neo_vm_rs() {
    let workspace = workspace_root();
    let external_vm =
        read_source(workspace.join("neo-smart-contract/src/application_engine/external_vm.rs"));

    assert!(
        external_vm.contains("handle_runtime_syscall"),
        "the external VM host should bridge supported runtime syscalls directly"
    );
    assert!(
        external_vm.contains("System.Runtime.GetTrigger")
            && external_vm.contains("System.Runtime.GetNetwork")
            && external_vm.contains("System.Runtime.GetAddressVersion")
            && external_vm.contains("System.Runtime.GetInvocationCounter")
            && external_vm.contains("System.Runtime.GetExecutingScriptHash")
            && external_vm.contains("System.Runtime.GetEntryScriptHash")
            && external_vm.contains("System.Runtime.GetCallingScriptHash")
            && external_vm.contains("System.Runtime.BurnGas")
            && external_vm.contains("System.Runtime.CheckWitness")
            && external_vm.contains("System.Runtime.GetRandom")
            && external_vm.contains("System.Runtime.CurrentSigners"),
        "simple no-argument runtime syscalls should be admitted into the neo-vm-rs path"
    );
    assert!(
        external_vm.contains("stack_value_as_i64") && external_vm.contains("runtime_burn_gas"),
        "stack-consuming runtime syscalls should decode arguments from neo-vm-rs StackValue \
         and delegate host accounting to ApplicationEngine"
    );
    assert!(
        external_vm.contains("pop_vm_bytes") && external_vm.contains("check_witness_hash"),
        "CheckWitness should decode a byte argument from neo-vm-rs StackValue and delegate \
         witness evaluation to ApplicationEngine"
    );

    let settings = ProtocolSettings::default();
    let mut builder = ScriptBuilder::new();
    builder.emit_push_int(2);
    builder
        .emit_syscall("System.Runtime.BurnGas")
        .expect("burn gas syscall");
    builder.emit_push(&[0u8; 20]);
    builder
        .emit_syscall("System.Runtime.CheckWitness")
        .expect("check witness syscall");
    builder
        .emit_syscall("System.Runtime.GetRandom")
        .expect("get random syscall");
    builder
        .emit_syscall("System.Runtime.CurrentSigners")
        .expect("current signers syscall");
    builder
        .emit_syscall("System.Runtime.GetTrigger")
        .expect("get trigger syscall");
    builder
        .emit_syscall("System.Runtime.GetNetwork")
        .expect("get network syscall");
    builder
        .emit_syscall("System.Runtime.GetAddressVersion")
        .expect("get address version syscall");
    builder
        .emit_syscall("System.Runtime.GetInvocationCounter")
        .expect("get invocation counter syscall");
    builder
        .emit_syscall("System.Runtime.GetExecutingScriptHash")
        .expect("get executing script hash syscall");
    builder
        .emit_syscall("System.Runtime.GetEntryScriptHash")
        .expect("get entry script hash syscall");
    builder
        .emit_syscall("System.Runtime.GetCallingScriptHash")
        .expect("get calling script hash syscall");
    builder.emit_opcode(OpCode::RET);
    let script = builder.to_array();
    let expected_script_hash = UInt160::from_script(&script).to_bytes();

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        settings.clone(),
        TEST_MODE_GAS,
        None,
        neo_native_contracts::native::composition::default_native_services(),
    )
    .expect("application engine");
    engine
        .load_script(script, CallFlags::NONE, None)
        .expect("load script");

    assert_eq!(engine.execute_allow_fault(), VMState::HALT);
    assert_eq!(engine.result_stack().len(), 10);
    assert!(engine.result_stack().peek(0).unwrap().is_null());
    assert_eq!(
        engine.result_stack().peek(1).unwrap().as_bytes().unwrap(),
        expected_script_hash
    );
    assert_eq!(
        engine.result_stack().peek(2).unwrap().as_bytes().unwrap(),
        expected_script_hash
    );
    assert_eq!(
        engine.result_stack().peek(3).unwrap().as_int().unwrap(),
        BigInt::from(1)
    );
    assert_eq!(
        engine.result_stack().peek(4).unwrap().as_int().unwrap(),
        BigInt::from(i64::from(settings.address_version))
    );
    assert_eq!(
        engine.result_stack().peek(5).unwrap().as_int().unwrap(),
        BigInt::from(settings.network as i64)
    );
    assert_eq!(
        engine.result_stack().peek(6).unwrap().as_int().unwrap(),
        BigInt::from(i64::from(TriggerType::Application.bits()))
    );
    assert!(engine.result_stack().peek(7).unwrap().is_null());
    assert!(engine.result_stack().peek(8).unwrap().as_int().is_ok());
    assert!(!engine.result_stack().peek(9).unwrap().as_bool().unwrap());
}

#[test]
fn application_engine_routes_script_container_through_neo_vm_rs() {
    let workspace = workspace_root();
    let external_vm =
        read_source(workspace.join("neo-smart-contract/src/application_engine/external_vm.rs"));
    assert!(
        external_vm.contains("System.Runtime.GetScriptContainer")
            && external_vm.contains("map(signer_to_stack_value)")
            && external_vm.contains("transaction_to_stack_value(transaction)"),
        "script container projection should flow through the protocol_projection \
         StackValue adapters"
    );

    let mut transaction = Transaction::new();
    transaction.set_script(vec![OpCode::PUSH1.byte()]);
    let account = UInt160::from_bytes(&[1u8; 20]).expect("account");
    transaction.add_signer(Signer::new(account, WitnessScope::NONE));
    let expected_hash = transaction.hash().to_bytes();

    let mut builder = ScriptBuilder::new();
    builder
        .emit_syscall("System.Runtime.GetScriptContainer")
        .expect("script container syscall");
    builder.emit_opcode(OpCode::RET);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(Arc::new(transaction)),
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
        neo_native_contracts::native::composition::default_native_services(),
    )
    .expect("application engine");
    engine
        .load_script(builder.to_array(), CallFlags::NONE, None)
        .expect("load script");

    assert_eq!(engine.execute_allow_fault(), VMState::HALT);
    let result = engine.result_stack().peek(0).expect("result item");
    let StackItem::Array(array) = result else {
        panic!("expected transaction stack array");
    };
    assert_eq!(array.len(), 8);
    assert_eq!(array.items()[0].as_bytes().unwrap(), expected_hash);
}

#[test]
fn application_engine_routes_runtime_log_through_neo_vm_rs() {
    let workspace = workspace_root();
    let external_vm =
        read_source(workspace.join("neo-smart-contract/src/application_engine/external_vm.rs"));
    assert!(
        external_vm.contains("System.Runtime.Log") && external_vm.contains("emit_log_event"),
        "Runtime.Log should be handled directly by the neo-vm-rs syscall host"
    );
    assert!(
        external_vm.contains("required_call_flags") && external_vm.contains("has_call_flags"),
        "the direct neo-vm-rs syscall host should enforce registered syscall call flags"
    );

    let mut transaction = Transaction::new();
    transaction.set_script(vec![OpCode::PUSH1.byte()]);

    let mut builder = ScriptBuilder::new();
    builder.emit_push_string("hello");
    builder
        .emit_syscall("System.Runtime.Log")
        .expect("runtime log syscall");
    builder.emit_opcode(OpCode::RET);

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(Arc::new(transaction)),
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
        neo_native_contracts::native::composition::default_native_services(),
    )
    .expect("application engine");
    engine
        .load_script(builder.to_array(), CallFlags::ALLOW_NOTIFY, None)
        .expect("load script");

    assert_eq!(engine.execute_allow_fault(), VMState::HALT);
    assert_eq!(engine.logs().len(), 1);
    assert_eq!(engine.logs()[0].message, "hello");
}

#[test]
fn execution_engine_limits_facade_is_removed() {
    let workspace = workspace_root();
    let limits_path = workspace.join("neo-core/src/neo_vm/execution_engine_limits.rs");
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));
    let engine_module = read_source(workspace.join("neo-vm/src/execution_engine/mod.rs"));
    let limits = ExecutionEngineLimits::default();

    assert!(
        !limits_path.exists()
            && !vm_module.contains("pub mod execution_engine_limits")
            && !vm_module.contains("pub use execution_engine_limits::ExecutionEngineLimits"),
        "neo-core should not keep a local ExecutionEngineLimits facade"
    );
    assert!(
        engine_module.contains("use crate::{ExecutionEngineLimits, VmState as VMState};"),
        "local execution engine internals should import ExecutionEngineLimits directly \
         from the neo-vm crate root"
    );
    assert_eq!(
        limits.max_stack_size,
        neo_vm::DEFAULT_MAX_STACK_DEPTH as u32,
        "ExecutionEngineLimits should use neo-vm-rs for the default stack depth"
    );
    assert_eq!(
        limits.max_invocation_stack_size,
        neo_vm::DEFAULT_MAX_INVOCATION_DEPTH as u32,
        "ExecutionEngineLimits should use neo-vm-rs for the default invocation depth"
    );
    // C# neo-vm v3.9.0 ExecutionEngineLimits: MaxItemSize = ushort.MaxValue * 2.
    assert_eq!(
        limits.max_item_size,
        u16::MAX as u32 * 2,
        "ExecutionEngineLimits must keep the 131070-byte item-size limit"
    );
    // C#: MaxComparableSize = 65536, i.e. u16::MAX + 1.
    assert_eq!(limits.max_comparable_size, u16::MAX as u32 + 1);
    assert_eq!(limits.max_shift, 256);
    assert_eq!(limits.max_try_nesting_depth, 16);
    assert!(limits.catch_engine_exceptions);
}

#[test]
fn exception_handling_facades_are_removed() {
    let workspace = workspace_root();
    let context_path = workspace.join("neo-core/src/neo_vm/exception_handling_context.rs");
    let state_path = workspace.join("neo-core/src/neo_vm/exception_handling_state.rs");
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));
    let execution_context = read_source(workspace.join("neo-vm/src/execution_context/context.rs"));
    let exception_runtime = read_source(workspace.join("neo-vm/src/execution_engine/exception.rs"));
    let mut context = ExceptionHandlingContext::new(-1, 42);

    assert!(
        !context_path.exists()
            && !state_path.exists()
            && !vm_module.contains("pub mod exception_handling_context")
            && !vm_module.contains("pub mod exception_handling_state"),
        "neo-rs should not keep local exception handling facade modules; the \
         ExceptionHandlingContext/State types are owned by the neo-vm crate"
    );
    assert!(
        execution_context.contains("use crate::ExceptionHandlingContext;")
            && execution_context.contains("use crate::ExceptionHandlingState;")
            && exception_runtime.contains("use crate::ExceptionHandlingContext;")
            && exception_runtime.contains("use crate::ExceptionHandlingState;"),
        "local runtime should import exception frame types directly from the \
         neo-vm crate root"
    );

    assert_eq!(context.catch_pointer(), -1);
    assert_eq!(context.finally_pointer(), 42);
    assert_eq!(context.state(), ExceptionHandlingState::Try);
    context.set_state(ExceptionHandlingState::Finally);
    assert!(context.is_in_exception());
}

#[test]
fn neo_core_does_not_reexport_opcode_through_vm_facade() {
    let workspace = workspace_root();
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));

    assert!(
        !vm_module.contains("pub use neo_vm::OpCode;"),
        "neo_core::neo_vm should not keep an OpCode facade; callers should import \
         neo_vm::OpCode directly"
    );
    assert!(
        !vm_module.contains("pub use op_code::OpCode;"),
        "neo_core::neo_vm should not route OpCode through a compatibility module"
    );
    assert!(
        !vm_module.contains("pub mod op_code;"),
        "neo_core::neo_vm should not expose a compatibility op_code module"
    );
    assert!(
        !workspace.join("neo-core/src/neo_vm/op_code").exists(),
        "neo-core/src/neo_vm/op_code should be deleted once OpCode comes from neo-vm-rs"
    );
}

#[test]
fn native_contract_static_syscall_hash_uses_neo_vm_rs_directly() {
    let workspace = workspace_root();
    let native_contract =
        read_source(workspace.join("neo-smart-contract/src/application_engine/native_contract.rs"));

    assert!(
        native_contract.contains("neo_vm::interop_hash(\"System.Contract.CallNative\")"),
        "static native contract syscall hashing should call neo_vm::interop_hash directly"
    );
    assert!(
        !native_contract.contains("ScriptBuilder::hash_syscall(\"System.Contract.CallNative\")"),
        "static native contract syscall hashing should not route through ScriptBuilder validation"
    );
}

#[test]
fn interop_service_hashes_syscalls_with_neo_vm_rs_directly() {
    let workspace = workspace_root();
    let interop_service = read_source(workspace.join("neo-vm/src/interop_service.rs"));

    assert!(
        interop_service.contains("crate::interop_hash(")
            || interop_service.contains("neo_vm::interop_hash"),
        "InteropService should compute syscall hashes with the crate's interop_hash directly"
    );
    assert!(
        !interop_service.contains("use crate::script_builder::ScriptBuilder")
            && !interop_service.contains("ScriptBuilder::hash_syscall"),
        "InteropService should not depend on script construction just to hash syscalls"
    );
}

#[test]
fn smart_contract_helper_hashes_syscalls_with_neo_vm_rs_directly() {
    let workspace = workspace_root();
    let helper = read_source(workspace.join("neo-smart-contract/src/helper.rs"));

    assert!(
        helper.contains("neo_vm::interop_hash(name).to_le_bytes()"),
        "smart_contract::Helper should use neo-vm-rs interop_hash for syscall IDs"
    );
    assert!(
        !helper.contains("Crypto::sha256(name.as_bytes())"),
        "smart_contract::Helper should not duplicate syscall hash derivation locally"
    );
}

#[test]
fn interop_descriptor_hashes_syscalls_with_neo_vm_rs_directly() {
    let workspace = workspace_root();
    let descriptor = read_source(workspace.join("neo-smart-contract/src/interop_descriptor.rs"));

    assert!(
        descriptor.contains("neo_vm::interop_hash(&self.name)"),
        "InteropDescriptor should use neo-vm-rs interop_hash for service hashes"
    );
    assert!(
        !descriptor.contains("Crypto::sha256(self.name.as_bytes())"),
        "InteropDescriptor should not duplicate syscall hash derivation locally"
    );
}

#[test]
fn script_builder_reuses_neo_vm_rs_integer_encoding_for_i64_pushes() {
    let workspace = workspace_root();
    // The real ScriptBuilder implementation lives in the neo-vm crate;
    // The core root exports the authoritative module without a forwarding file.
    let script_builder = read_source(workspace.join("neo-vm/src/script_builder.rs"));

    assert!(
        script_builder.matches("crate::encode_integer").count() >= 2,
        "ScriptBuilder should reuse the crate's integer encoding for direct i64 pushes \
         and for i64-sized BigInt pushes"
    );
    assert!(
        !script_builder.contains("let buf = value.to_le_bytes();"),
        "ScriptBuilder::emit_push_int should not duplicate neo-vm-rs integer encoding"
    );
    assert!(
        !script_builder.contains("let mut bytes = value.to_signed_bytes_le();"),
        "ScriptBuilder::emit_push_bigint should not duplicate integer encoding for \
         values that fit in i64"
    );
}

#[test]
fn script_builder_emits_opcode_bytes_via_neo_vm_rs_opcode_metadata() {
    let workspace = workspace_root();
    let script_builder = read_source(workspace.join("neo-vm/src/script_builder.rs"));

    assert!(
        script_builder.contains("op.byte()"),
        "ScriptBuilder's central opcode emitter should use OpCode::byte()"
    );
    assert!(
        !script_builder.contains("self.script.push(op as u8)"),
        "ScriptBuilder should not hand-cast OpCode in its central emitter"
    );
}

#[test]
fn script_builder_derives_opcode_bytes_from_neo_vm_rs_metadata() {
    let workspace = workspace_root();
    let script_builder = read_source(workspace.join("neo-vm/src/script_builder.rs"));

    assert!(
        script_builder.contains("OpCode::PUSH0.byte()"),
        "ScriptBuilder should derive small-integer PUSH opcodes from \
         OpCode::byte() metadata"
    );
    for cast in [
        "OpCode::PUSH0 as u8",
        "OpCode::JMP as u8",
        "OpCode::JMPLE_L as u8",
        "let opcode_value = opcode as u8",
    ] {
        assert!(
            !script_builder.contains(cast),
            "ScriptBuilder should use OpCode byte metadata instead of {cast}"
        );
    }
}

#[test]
fn vm_dispatch_uses_neo_vm_rs_opcode_byte_metadata() {
    let workspace = workspace_root();
    let jump_table = read_source(workspace.join("neo-vm/src/jump_table/mod.rs"));
    let execution = read_source(workspace.join("neo-vm/src/execution_engine/execution.rs"));

    assert!(
        jump_table.contains("usize::from(opcode.byte())"),
        "JumpTable should index handlers through neo-vm-rs OpCode::byte() metadata"
    );
    assert!(
        !jump_table.contains("opcode as usize"),
        "JumpTable should not rely on local enum representation casts for handler indexing"
    );
    assert!(
        execution.contains("opcode.byte()") && !execution.contains("opcode as u8"),
        "ExecutionEngine dispatch should pass the neo-vm-rs opcode byte directly"
    );
}

#[test]
fn script_builder_emit_syscall_hashes_with_neo_vm_rs_directly() {
    let workspace = workspace_root();
    let script_builder = read_source(workspace.join("neo-vm/src/script_builder.rs"));

    assert!(
        script_builder.contains("crate::interop_hash(api)")
            || script_builder.contains("neo_vm::interop_hash(api)"),
        "ScriptBuilder::emit_syscall should hash with the crate's interop_hash directly"
    );
    assert!(
        !script_builder.contains("pub fn hash_syscall")
            && !script_builder.contains("Self::hash_syscall")
            && !script_builder.contains("ScriptBuilder::hash_syscall"),
        "ScriptBuilder should not expose a redundant syscall-hash wrapper; callers can \
         use neo_vm::interop_hash directly"
    );
}

#[test]
fn witness_rules_project_through_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    // The witness-rule stack projection moved out of neo-core: the canonical
    // trait projection lives in neo-vm and the mirror copy in the
    // neo-smart-contract authority crate; guard both so the projection stays
    // StackValue-first.
    let witness_rule = [
        read_source(workspace.join("neo-vm/src/stack_projection.rs")),
        read_source(workspace.join("neo-smart-contract/src/protocol_projection/witness_rule.rs")),
    ]
    .join("\n");

    assert!(
        witness_rule.contains("use neo_vm::StackValue;")
            || witness_rule.contains("use neo_vm::{StackItem, StackValue};"),
        "witness rules should import StackValue through the neo-vm boundary for their \
         data projection"
    );
    assert!(
        witness_rule
            .matches("fn to_stack_value(&self) -> StackValue")
            .count()
            >= 2,
        "WitnessCondition and WitnessRule should both expose direct StackValue projections"
    );
    assert!(
        witness_rule
            .matches("StackItem::try_from(self.to_stack_value())")
            .count()
            + witness_rule
                .matches("stack_value_to_item(self.to_stack_value())")
                .count()
            >= 2,
        "WitnessCondition::to_stack_item and WitnessRule::to_stack_item should adapt \
         from the direct StackValue projection"
    );
    for required in [
        "pub fn witness_condition_to_stack_value(condition: &WitnessCondition) -> StackValue",
        "pub fn witness_rule_to_stack_value(rule: &WitnessRule) -> StackValue",
        "condition.condition_type().to_byte()",
        "items.push(StackValue::Boolean(*value))",
        "items.push(witness_condition_to_stack_value(condition))",
        ".map(witness_condition_to_stack_value)",
        "StackValue::ByteString(hash.to_bytes())",
        "StackValue::ByteString(group.clone())",
        "StackValue::Integer(i64::from(rule.action.to_byte()))",
        "witness_condition_to_stack_value(&rule.condition)",
    ] {
        assert!(
            witness_rule.contains(required),
            "missing recursive rule projection: {required}"
        );
    }
    for local_builder in [
        "StackItem::from_array",
        "StackItem::from_int",
        "StackItem::from_bool",
        "StackItem::from_byte_string",
    ] {
        assert!(
            !witness_rule.contains(local_builder),
            "witness rules should not hand-build stack-item shapes with {local_builder}; \
             build neo_vm::StackValue first"
        );
    }
}

#[test]
fn contract_parameter_definition_projection_uses_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let parameter_definition = fs::read_to_string(
        workspace.join("neo-smart-contract/src/manifest/contract_parameter_definition.rs"),
    )
    .unwrap();

    assert!(
        parameter_definition.contains("use neo_vm::StackValue;"),
        "ContractParameterDefinition should import neo_vm::StackValue directly for \
         its manifest data projection"
    );
    assert!(
        parameter_definition.contains("pub fn to_stack_value(&self) -> StackValue"),
        "ContractParameterDefinition should expose a direct StackValue projection"
    );
    assert!(
        parameter_definition.contains(
            "pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError>"
        ),
        "ContractParameterDefinition should parse direct StackValue inputs before adapting \
         local StackItem"
    );
    assert!(
        parameter_definition.contains("StackItem::try_from(self.to_stack_value())"),
        "ContractParameterDefinition::to_stack_item should adapt from the direct \
         neo-vm-rs StackValue projection"
    );
    assert!(
        parameter_definition.contains("StackValue::try_from(stack_item)"),
        "ContractParameterDefinition::from_stack_item should adapt into neo-vm-rs \
         StackValue before parsing"
    );
    for local_builder in [
        "StackItem::from_struct",
        "StackItem::from_int",
        "StackItem::from_byte_string",
    ] {
        assert!(
            !parameter_definition.contains(local_builder),
            "ContractParameterDefinition should not hand-build stack-item shapes with \
             {local_builder}; build neo_vm::StackValue first"
        );
    }
}

#[test]
fn wild_card_container_projection_uses_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let wild_card_container = fs::read_to_string(
        workspace.join("neo-smart-contract/src/manifest/wild_card_container.rs"),
    )
    .unwrap();

    assert!(
        wild_card_container.contains("use neo_vm::StackValue;"),
        "WildCardContainer should import neo_vm::StackValue directly for \
         its manifest data projection"
    );
    assert!(
        wild_card_container.contains("pub fn to_stack_value(&self) -> StackValue"),
        "WildCardContainer<String> should expose a direct StackValue projection"
    );
    assert!(
        wild_card_container
            .contains("pub fn from_stack_value(stack_value: StackValue) -> Result<Self, String>"),
        "WildCardContainer<String> should parse direct StackValue inputs before adapting \
         local StackItem"
    );
    assert!(
        wild_card_container.contains("StackItem::try_from(self.to_stack_value())"),
        "WildCardContainer<String>::to_stack_item should adapt from the direct \
         neo-vm-rs StackValue projection"
    );
    assert!(
        wild_card_container.contains("StackValue::try_from(item.clone())"),
        "WildCardContainer<String>::from_stack_item should adapt into neo-vm-rs \
         StackValue before parsing"
    );
    for local_builder in ["StackItem::from_array", "StackItem::from_byte_string"] {
        assert!(
            !wild_card_container.contains(local_builder),
            "WildCardContainer<String> should not hand-build stack-item shapes with \
             {local_builder}; build neo_vm::StackValue first"
        );
    }
}

#[test]
fn manifest_event_and_method_descriptors_project_through_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let event_descriptor = fs::read_to_string(
        workspace.join("neo-smart-contract/src/manifest/contract_event_descriptor.rs"),
    )
    .unwrap();
    let method_descriptor = fs::read_to_string(
        workspace.join("neo-smart-contract/src/manifest/contract_method_descriptor.rs"),
    )
    .unwrap();

    for (name, source) in [
        ("ContractEventDescriptor", event_descriptor.as_str()),
        ("ContractMethodDescriptor", method_descriptor.as_str()),
    ] {
        assert!(
            source.contains("use neo_vm::StackValue;"),
            "{name} should import neo_vm::StackValue directly for manifest projection"
        );
        assert!(
            source.contains("pub fn to_stack_value(&self) -> StackValue"),
            "{name} should expose a direct StackValue projection"
        );
        assert!(
            source
                .contains("pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError>"),
            "{name} should parse direct StackValue inputs before adapting local StackItem"
        );
        assert!(
            source.contains("StackItem::try_from(self.to_stack_value())"),
            "{name}::to_stack_item should adapt from the direct neo-vm-rs StackValue projection"
        );
        assert!(
            source.contains("StackValue::try_from(stack_item)"),
            "{name}::from_stack_item should adapt into neo-vm-rs StackValue before parsing"
        );
        for local_builder in [
            "StackItem::from_struct",
            "StackItem::from_array",
            "StackItem::from_int",
            "StackItem::from_bool",
            "StackItem::from_byte_string",
        ] {
            assert!(
                !source.contains(local_builder),
                "{name} should not hand-build stack-item shapes with {local_builder}; build \
                 neo_vm::StackValue first"
            );
        }
    }
}

#[test]
fn manifest_permissions_project_through_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let permission_descriptor = fs::read_to_string(
        workspace.join("neo-smart-contract/src/manifest/contract_permission_descriptor.rs"),
    )
    .unwrap();
    let permission = fs::read_to_string(
        workspace.join("neo-smart-contract/src/manifest/contract_permission.rs"),
    )
    .unwrap();

    assert!(
        permission_descriptor.contains("use neo_vm::StackValue;"),
        "ContractPermissionDescriptor should import neo_vm::StackValue directly for \
         manifest projection"
    );
    assert!(
        permission_descriptor.contains("pub fn to_stack_value(&self) -> StackValue"),
        "ContractPermissionDescriptor should expose a direct StackValue projection"
    );
    assert!(
        permission_descriptor
            .contains("pub fn from_stack_value(stack_value: StackValue) -> Result<Self, String>"),
        "ContractPermissionDescriptor should parse direct StackValue inputs before adapting \
         local StackItem"
    );
    assert!(
        permission_descriptor.contains("StackItem::try_from(self.to_stack_value())"),
        "ContractPermissionDescriptor::to_stack_item should adapt from direct StackValue"
    );
    assert!(
        permission_descriptor.contains("StackValue::try_from(item.clone())"),
        "ContractPermissionDescriptor::from_stack_item should adapt into StackValue first"
    );
    for local_builder in ["StackItem::null", "StackItem::from_byte_string"] {
        assert!(
            !permission_descriptor.contains(local_builder),
            "ContractPermissionDescriptor should not hand-build stack-item shapes with \
             {local_builder}; build neo_vm::StackValue first"
        );
    }

    assert!(
        permission.contains("use neo_vm::StackValue;"),
        "ContractPermission should import neo_vm::StackValue directly for manifest projection"
    );
    assert!(
        permission.contains("pub fn to_stack_value(&self) -> StackValue"),
        "ContractPermission should expose a direct StackValue projection"
    );
    assert!(
        permission.contains(
            "pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError>"
        ),
        "ContractPermission should parse direct StackValue inputs before adapting local StackItem"
    );
    assert!(
        permission.contains("StackItem::try_from(self.to_stack_value())"),
        "ContractPermission::to_stack_item should adapt from direct StackValue"
    );
    assert!(
        permission.contains("StackValue::try_from(stack_item)"),
        "ContractPermission::from_stack_item should adapt into StackValue first"
    );
    assert!(
        !permission.contains("StackItem::from_struct"),
        "ContractPermission should not hand-build StackItem structs; build neo_vm::StackValue first"
    );
}

#[test]
fn contract_group_projects_through_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let contract_group =
        fs::read_to_string(workspace.join("neo-smart-contract/src/manifest/contract_group.rs"))
            .unwrap();

    assert!(
        contract_group.contains("use neo_vm::StackValue;"),
        "ContractGroup should import neo_vm::StackValue directly for manifest projection"
    );
    assert!(
        contract_group.contains("pub fn to_stack_value(&self) -> StackValue"),
        "ContractGroup should expose a direct StackValue projection"
    );
    assert!(
        contract_group
            .contains("pub fn try_from_stack_value(stack_value: StackValue) -> Result<Self>"),
        "ContractGroup should parse direct StackValue inputs before adapting local StackItem"
    );
    assert!(
        contract_group.contains("StackItem::try_from(self.to_stack_value())"),
        "ContractGroup::to_stack_item should adapt from direct StackValue"
    );
    assert!(
        contract_group.contains("StackValue::try_from(stack_item)"),
        "ContractGroup::from_stack_item should adapt into StackValue first"
    );
    assert!(
        contract_group.contains("StackValue::try_from(stack_item.clone())"),
        "ContractGroup::try_from_stack_item_value should adapt borrowed local StackItems \
         into StackValue before parsing"
    );
    for local_builder in [
        "StackItem::from_struct",
        "StackItem::from_array",
        "StackItem::from_int",
        "StackItem::from_byte_string",
    ] {
        assert!(
            !contract_group.contains(local_builder),
            "ContractGroup should not hand-build stack-item shapes with {local_builder}; build \
             neo_vm::StackValue first"
        );
    }
}

#[test]
fn contract_abi_projects_through_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let contract_abi =
        read_source(workspace.join("neo-smart-contract/src/manifest/contract_abi.rs"));

    assert!(
        contract_abi.contains("use neo_vm::StackValue;"),
        "ContractAbi should import neo_vm::StackValue directly for manifest projection"
    );
    assert!(
        contract_abi.contains("pub fn to_stack_value(&self) -> StackValue"),
        "ContractAbi should expose a direct StackValue projection"
    );
    assert!(
        contract_abi.contains(
            "pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError>"
        ),
        "ContractAbi should parse direct StackValue inputs before adapting local StackItem"
    );
    assert!(
        contract_abi.contains("self.method_dictionary = None;"),
        "ContractAbi::from_stack_value should invalidate the method cache after replacing methods"
    );
    assert!(
        contract_abi.contains("StackItem::try_from(self.to_stack_value())"),
        "ContractAbi::to_stack_item should adapt from direct StackValue"
    );
    assert!(
        contract_abi.contains("StackValue::try_from(stack_item)"),
        "ContractAbi::from_stack_item should adapt into StackValue first"
    );
    for local_builder in ["StackItem::from_struct", "StackItem::from_array"] {
        assert!(
            !contract_abi.contains(local_builder),
            "ContractAbi should not hand-build stack-item shapes with {local_builder}; build \
             neo_vm::StackValue first"
        );
    }
}

#[test]
fn contract_manifest_and_state_project_through_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let manifest =
        fs::read_to_string(workspace.join("neo-smart-contract/src/manifest/contract_manifest.rs"))
            .unwrap();
    let contract_state = read_source(workspace.join("neo-smart-contract/src/contract_state.rs"));

    assert!(
        manifest.contains("use neo_vm::StackValue;"),
        "ContractManifest should import neo_vm::StackValue directly for metadata projection"
    );
    assert!(
        manifest.contains("pub fn to_stack_value(&self) -> StackValue"),
        "ContractManifest should expose a direct StackValue projection"
    );
    assert!(
        manifest.contains("pub fn from_stack_value(&mut self, stack_value: StackValue)"),
        "ContractManifest should parse direct StackValue inputs before adapting local StackItem"
    );
    assert!(
        manifest.contains("StackItem::try_from(self.to_stack_value())"),
        "ContractManifest::to_stack_item should adapt from direct StackValue"
    );
    assert!(
        manifest.contains("StackValue::try_from(stack_item)"),
        "ContractManifest::from_stack_item should adapt into StackValue first"
    );
    for local_builder in [
        "StackItem::from_struct",
        "StackItem::from_array",
        "StackItem::from_int",
        "StackItem::from_bool",
        "StackItem::from_byte_string",
    ] {
        assert!(
            !manifest.contains(local_builder),
            "ContractManifest should not hand-build stack-item shapes with {local_builder}; \
             build neo_vm::StackValue first"
        );
    }

    assert!(
        contract_state.contains("use neo_vm::StackValue;"),
        "ContractState should import neo_vm::StackValue directly for metadata projection"
    );
    assert!(
        contract_state.contains("pub fn to_stack_value(&self) -> StackValue"),
        "ContractState should expose a direct StackValue projection"
    );
    assert!(
        contract_state.contains("pub fn from_stack_value(&mut self, stack_value: StackValue)"),
        "ContractState should parse direct StackValue inputs before adapting local StackItem"
    );
    assert!(
        contract_state.contains("self.manifest.to_stack_value()")
            && contract_state.contains("manifest.from_stack_value(items[4].clone())"),
        "ContractState should compose ContractManifest through StackValue, not local StackItem"
    );
    assert!(
        contract_state.contains("StackItem::try_from(self.to_stack_value())"),
        "ContractState::to_stack_item should adapt from direct StackValue"
    );
    assert!(
        contract_state.contains("StackValue::try_from(stack_item)"),
        "ContractState::from_stack_item should adapt into StackValue first"
    );
    for local_builder in [
        "StackItem::from_array",
        "StackItem::from_int",
        "StackItem::from_byte_string",
    ] {
        assert!(
            !contract_state.contains(local_builder),
            "ContractState should not hand-build stack-item shapes with {local_builder}; \
             build neo_vm::StackValue first"
        );
    }
}

#[test]
fn contract_management_persisted_contract_state_uses_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let source = fs::read_to_string(
        workspace.join("neo-native-contracts/src/native/contract_management/mod.rs"),
    )
    .unwrap();
    let storage_write_section = source
        .split("pub(super) fn serialize_contract_state")
        .nth(1)
        .and_then(|tail| tail.split("pub fn deserialize_contract_state").next())
        .expect("contract-management ContractState storage write section");
    let storage_read_section = source
        .split("pub fn deserialize_contract_state")
        .nth(1)
        .and_then(|tail| tail.split("fn invoke_deploy_hook").next())
        .expect("contract-management ContractState storage read section");

    assert!(
        source.contains("use neo_vm::StackValue;")
            || source.contains("use neo_vm::{ExecutionEngineLimits, StackValue};"),
        "ContractManagement persisted ContractState storage should import neo_vm::StackValue"
    );
    assert!(
        storage_write_section.contains("contract.to_stack_value()")
            && storage_write_section.contains("BinarySerializer::serialize_stack_value"),
        "ContractManagement persisted ContractState writes should serialize direct StackValue"
    );
    assert!(
        storage_read_section.contains("BinarySerializer::deserialize_stack_value(bytes)")
            && storage_read_section.contains("contract.from_stack_value(value)"),
        "ContractManagement persisted ContractState reads should deserialize direct StackValue"
    );
    assert!(
        !storage_write_section.contains("contract.to_stack_item()")
            && !storage_write_section.contains("BinarySerializer::serialize("),
        "ContractManagement persisted ContractState writes should not bounce through StackItem"
    );
    assert!(
        !storage_read_section.contains("BinarySerializer::deserialize(bytes")
            && !storage_read_section.contains("contract_state_from_stack_item")
            && !source.contains("fn contract_state_from_stack_item"),
        "ContractManagement persisted ContractState reads should not parse through local StackItem"
    );
}

#[test]
fn contract_management_manifest_validation_uses_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let source = fs::read_to_string(
        workspace.join("neo-native-contracts/src/native/contract_management/validation.rs"),
    )
    .unwrap();
    let validation_section = source
        .split("pub(super) fn validate_manifest_serialization")
        .nth(1)
        .and_then(|tail| tail.split("pub(super) fn validate_script_and_abi").next())
        .expect("contract-management manifest serialization validation section");

    assert!(
        validation_section.contains("BinarySerializer::serialize_stack_value")
            && validation_section.contains("manifest.to_stack_value()"),
        "ContractManagement manifest validation should serialize ContractManifest through direct \
         neo_vm::StackValue projection"
    );
    assert!(
        !validation_section.contains("manifest.to_stack_item()")
            && !validation_section.contains("BinarySerializer::serialize(")
            && !source.contains("use crate::smart_contract::interoperable::Interoperable;"),
        "ContractManagement manifest validation should not bounce through local StackItem or \
         Interoperable"
    );
}

#[test]
fn native_pure_data_states_project_through_neo_vm_rs_stack_value() {
    let workspace = workspace_root();

    let transaction_state =
        fs::read_to_string(workspace.join("neo-native-contracts/src/native/transaction_state.rs"))
            .unwrap();
    assert!(
        transaction_state.contains("use neo_vm::StackValue;")
            || transaction_state.contains("use neo_vm::{StackValue, VmState as VMState};"),
        "TransactionState should import neo_vm::StackValue directly for native data projection"
    );
    assert!(
        transaction_state.contains("pub fn to_stack_value(&self) -> StackValue"),
        "TransactionState should expose a direct StackValue projection"
    );
    assert!(
        transaction_state.contains(
            "pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError>"
        ),
        "TransactionState should parse direct StackValue inputs"
    );
    assert!(
        !transaction_state.contains("use crate::smart_contract::interoperable::Interoperable")
            && !transaction_state.contains("impl Interoperable for TransactionState")
            && !transaction_state.contains("from_stack_item")
            && !transaction_state.contains("to_stack_item")
            && !transaction_state.contains("StackItem::try_from")
            && !transaction_state.contains("StackValue::try_from(stack_item)"),
        "TransactionState is pure persisted ledger data and should use StackValue directly, \
         without local StackItem adapters"
    );
    for local_builder in [
        "StackItem::from_struct",
        "StackItem::from_array",
        "StackItem::from_int",
        "StackItem::from_byte_string",
    ] {
        assert!(
            !transaction_state.contains(local_builder),
            "TransactionState should not hand-build stack-item shapes with {local_builder}; build \
             neo_vm::StackValue first"
        );
    }

    let oracle_request =
        read_source(workspace.join("neo-native-contracts/src/native/oracle_request.rs"));
    let oracle_storage = fs::read_to_string(
        workspace.join("neo-native-contracts/src/native/oracle_contract/storage.rs"),
    )
    .unwrap();
    assert!(
        oracle_request.contains("use neo_vm::StackValue;"),
        "OracleRequest should import neo_vm::StackValue directly for native data projection"
    );
    assert!(
        oracle_request.contains("pub fn to_stack_value(&self) -> StackValue"),
        "OracleRequest should expose a direct StackValue projection"
    );
    assert!(
        oracle_request.contains(
            "pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError>"
        ),
        "OracleRequest should parse direct StackValue inputs without depending on local StackItem"
    );
    assert!(
        !oracle_request.contains("use crate::neo_vm::StackItem")
            && !oracle_request.contains("impl Interoperable for OracleRequest")
            && !oracle_request.contains("from_stack_item")
            && !oracle_request.contains("to_stack_item"),
        "OracleRequest is pure native request data and should not keep local StackItem adapters"
    );
    for local_builder in [
        "StackItem::from_array",
        "StackItem::from_int",
        "StackItem::from_byte_string",
        "StackItem::null",
    ] {
        assert!(
            !oracle_request.contains(local_builder),
            "OracleRequest should not hand-build stack-item shapes with {local_builder}; build \
             neo_vm::StackValue first"
        );
    }
    let oracle_request_storage_section = oracle_storage
        .split("fn read_id_list")
        .next()
        .expect("Oracle request storage section");
    assert!(
        oracle_request_storage_section.contains("use neo_vm::StackValue;")
            && oracle_request_storage_section.contains("BinarySerializer::serialize_stack_value")
            && oracle_request_storage_section.contains("BinarySerializer::deserialize_stack_value")
            && oracle_request_storage_section.contains("OracleRequest::new(")
            && oracle_request_storage_section.contains(".to_stack_value()"),
        "Oracle request storage should serialize/deserialize direct neo_vm::StackValue \
         projections"
    );
    assert!(
        !oracle_request_storage_section.contains("BinarySerializer::serialize(&stack_item")
            && !oracle_request_storage_section.contains("let stack_item =")
            && !oracle_request_storage_section.contains("StackItem::from_array(vec![")
            && !oracle_request_storage_section
                .contains("StackItem::from_byte_string(request.original_tx_id.to_bytes())"),
        "Oracle request storage is pure persisted data and should not hand-build a local \
         StackItem array"
    );

    let trimmed_block =
        read_source(workspace.join("neo-native-contracts/src/native/trimmed_block.rs"));
    assert!(
        trimmed_block.contains("use neo_vm::StackValue;"),
        "TrimmedBlock should import neo_vm::StackValue directly for native data projection"
    );
    assert!(
        trimmed_block.contains("pub fn to_stack_value(&self) -> StackValue"),
        "TrimmedBlock should expose a direct StackValue projection"
    );
    assert!(
        trimmed_block.contains("StackItem::try_from(self.to_stack_value())"),
        "TrimmedBlock::to_stack_item should adapt from direct StackValue"
    );
    for local_builder in [
        "StackItem::from_array",
        "StackItem::from_int",
        "StackItem::from_byte_string",
    ] {
        assert!(
            !trimmed_block.contains(local_builder),
            "TrimmedBlock should not hand-build stack-item shapes with {local_builder}; build \
             neo_vm::StackValue first"
        );
    }

    let deposit_section =
        read_source(workspace.join("neo-native-contracts/src/native/notary/deposit.rs"));
    assert!(
        deposit_section.contains("use neo_vm::StackValue;"),
        "Notary Deposit should import neo_vm::StackValue directly for native data projection"
    );
    assert!(
        deposit_section.contains("pub fn to_stack_value(&self) -> StackValue")
            && deposit_section.contains("pub fn from_stack_value")
            && deposit_section.contains("stack_value: StackValue"),
        "Notary Deposit should expose direct StackValue projection and parsing"
    );
    assert!(
        !deposit_section.contains("impl Interoperable for Deposit")
            && !deposit_section.contains("from_stack_item")
            && !deposit_section.contains("to_stack_item"),
        "Notary Deposit uses custom persisted encoding and should not keep local StackItem \
         adapters"
    );

    let hash_index_state =
        fs::read_to_string(workspace.join("neo-native-contracts/src/native/hash_index_state.rs"))
            .unwrap();
    assert!(
        hash_index_state.contains("use neo_vm::StackValue;"),
        "HashIndexState should import neo_vm::StackValue directly for native data projection"
    );
    assert!(
        hash_index_state.contains("pub fn to_stack_value(&self) -> StackValue")
            && hash_index_state.contains("pub fn from_stack_value"),
        "HashIndexState should expose direct StackValue projection and parsing"
    );
    assert!(
        !hash_index_state.contains("use crate::neo_vm::StackItem")
            && !hash_index_state.contains("impl Interoperable for HashIndexState")
            && !hash_index_state.contains("from_stack_item")
            && !hash_index_state.contains("to_stack_item"),
        "HashIndexState is pure ledger data and should not keep local StackItem adapters"
    );

    let account_state =
        read_source(workspace.join("neo-native-contracts/src/native/account_state.rs"));
    let gas_token = read_source(workspace.join("neo-native-contracts/src/native/gas_token/mod.rs"));
    assert!(
        account_state.contains("use neo_vm::StackValue;"),
        "native AccountState should import neo_vm::StackValue directly for native data \
         projection"
    );
    assert!(
        account_state.contains("pub fn to_stack_value(&self) -> StackValue")
            && account_state.contains("pub fn from_stack_value"),
        "native AccountState should expose direct StackValue projection and parsing"
    );
    assert!(
        !account_state.contains("use crate::neo_vm::StackItem")
            && !account_state.contains("impl Interoperable for AccountState")
            && !account_state.contains("from_stack_item")
            && !account_state.contains("to_stack_item"),
        "native AccountState is pure persisted GAS data and should not keep local StackItem \
         adapters"
    );
    assert!(
        gas_token.contains("BinarySerializer::deserialize_stack_value(bytes.as_ref())")
            && gas_token.contains("state.from_stack_value(stack_value)")
            && !gas_token.contains("state.from_stack_item(stack_item)"),
        "GAS account state reads should deserialize directly into neo_vm::StackValue"
    );
    assert!(
        (gas_token.contains("BinarySerializer::serialize_stack_value")
            || gas_token.contains("serialize_stack_value_native(&state.to_stack_value())"))
            && gas_token.contains("&state.to_stack_value()")
            && !gas_token.contains("&state.to_stack_item()?"),
        "GAS account state writes should serialize direct StackValue projections"
    );

    let policy_contract = fs::read_to_string(
        workspace.join("neo-native-contracts/src/native/policy_contract/mod.rs"),
    )
    .unwrap();
    let policy_whitelist_section = policy_contract
        .split("/// Whitelisted fee contract info.")
        .nth(1)
        .and_then(|tail| tail.split("/// The Policy native contract.").next())
        .expect("Policy WhitelistedContract section");
    assert!(
        policy_contract.contains("use neo_vm::StackValue;")
            || policy_contract.contains("use neo_vm::{ExecutionEngineLimits, StackValue};"),
        "Policy WhitelistedContract should import neo_vm::StackValue directly for \
         native policy data projection"
    );
    assert!(
        policy_contract.contains("pub fn to_stack_value(&self) -> StackValue"),
        "Policy WhitelistedContract should expose a direct StackValue projection"
    );
    assert!(
        policy_contract
            .contains("pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<()>"),
        "Policy WhitelistedContract should parse direct StackValue inputs before adapting \
         local StackItem"
    );
    assert!(
        !policy_whitelist_section.contains("impl Interoperable for WhitelistedContract")
            && !policy_whitelist_section.contains("from_stack_item")
            && !policy_whitelist_section.contains("to_stack_item")
            && !policy_whitelist_section.contains("StackItem::try_from"),
        "Policy WhitelistedContract is pure persisted policy data and should not keep local \
         StackItem adapters"
    );
    for local_builder in [
        "StackItem::from_struct",
        "StackItem::from_array",
        "StackItem::from_int",
        "StackItem::from_byte_string",
    ] {
        assert!(
            !policy_contract.contains(local_builder),
            "Policy WhitelistedContract should not hand-build stack-item shapes with \
             {local_builder}; build neo_vm::StackValue first"
        );
    }
}

#[test]
fn policy_whitelist_storage_uses_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let policy_contract = fs::read_to_string(
        workspace.join("neo-native-contracts/src/native/policy_contract/mod.rs"),
    )
    .unwrap();
    let setters = fs::read_to_string(
        workspace.join("neo-native-contracts/src/native/policy_contract/setters.rs"),
    )
    .unwrap();
    let snapshot = fs::read_to_string(
        workspace.join("neo-native-contracts/src/native/policy_contract/snapshot.rs"),
    )
    .unwrap();
    let set_section = setters
        .split("pub(super) fn set_whitelist_fee_contract")
        .nth(1)
        .and_then(|tail| {
            tail.split("pub(super) fn remove_whitelist_fee_contract")
                .next()
        })
        .expect("policy whitelist setter section");
    let clean_section = setters
        .split("pub(crate) fn clean_whitelist")
        .nth(1)
        .expect("policy whitelist clean section");
    let snapshot_section = snapshot
        .split("pub fn get_whitelisted_fee")
        .nth(1)
        .expect("policy whitelist snapshot reader section");

    assert!(
        policy_contract.contains("fn serialize_whitelisted_contract")
            && policy_contract.contains("BinarySerializer::serialize_stack_value")
            && policy_contract.contains("whitelisted.to_stack_value()"),
        "Policy whitelist storage writes should serialize direct StackValue projections"
    );
    assert!(
        policy_contract.contains("fn deserialize_whitelisted_contract")
            && policy_contract.contains("BinarySerializer::deserialize_stack_value(bytes)")
            && policy_contract.contains("whitelist.from_stack_value(stack_value)"),
        "Policy whitelist storage reads should deserialize direct StackValue projections"
    );
    assert!(
        set_section.contains("Self::serialize_whitelisted_contract(&whitelisted)")
            && !set_section.contains("whitelisted.to_stack_item()")
            && !set_section.contains("BinarySerializer::serialize("),
        "Policy setWhitelistFeeContract should not persist through local StackItem"
    );
    assert!(
        clean_section.contains("Self::deserialize_whitelisted_contract(&bytes)")
            && !clean_section.contains("BinarySerializer::deserialize(")
            && !clean_section.contains("from_stack_item(stack_item)"),
        "Policy clean_whitelist should not read persisted whitelist entries through local StackItem"
    );
    assert!(
        snapshot_section.contains("Self::deserialize_whitelisted_contract(&bytes)")
            && !snapshot_section.contains("BinarySerializer::deserialize(")
            && !snapshot_section.contains("from_stack_item(stack_item)"),
        "Policy get_whitelisted_fee should not read persisted whitelist entries through local \
         StackItem"
    );
}

#[test]
fn neo_token_states_project_through_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let types = read_source(workspace.join("neo-native-contracts/src/native/neo_token/types.rs"));
    let native_impl = fs::read_to_string(
        workspace.join("neo-native-contracts/src/native/neo_token/native_impl.rs"),
    )
    .unwrap();
    let governance = fs::read_to_string(
        workspace.join("neo-native-contracts/src/native/neo_token/governance.rs"),
    )
    .unwrap();
    let nep17 = read_source(workspace.join("neo-native-contracts/src/native/neo_token/nep17.rs"));
    let native_account_projection = native_impl
        .split("impl NativeContract for NeoToken")
        .next()
        .expect("NeoAccountState helper section");

    assert!(
        types.contains("use neo_vm::StackValue;"),
        "NeoToken state structs should import neo_vm::StackValue directly for native \
         data projection"
    );
    assert!(
        types
            .matches("pub(super) fn to_stack_value(&self) -> StackValue")
            .count()
            >= 2,
        "NeoAccountState and CandidateState should expose direct StackValue projections"
    );
    assert!(
        types
            .matches("pub(super) fn from_stack_value(value: StackValue) -> Result<Self, String>")
            .count()
            >= 2,
        "NeoAccountState and CandidateState should parse direct StackValue inputs before \
         adapting local StackItem"
    );
    assert!(
        !types.contains("from_stack_item(item: StackItem)")
            && !types.contains("to_stack_item(&self) -> StackItem")
            && !native_impl.contains("to_stack_item(&self) -> StackItem"),
        "NeoToken pure persisted state should not keep local StackItem adapters once storage \
         reads/writes use direct StackValue projections"
    );
    assert!(
        types.contains("BinarySerializer::deserialize_stack_value(bytes)")
            && !types.contains("BinarySerializer::deserialize(bytes"),
        "NeoToken persisted state reads should deserialize directly into neo_vm::StackValue"
    );
    for (name, source) in [
        ("native_impl.rs", native_impl.as_str()),
        ("governance.rs", governance.as_str()),
        ("nep17.rs", nep17.as_str()),
    ] {
        assert!(
            (source.contains("BinarySerializer::serialize_stack_value")
                || source.contains("serialize_stack_value_native(&state.to_stack_value())"))
                && source.contains("&state.to_stack_value()"),
            "{name} should serialize NeoToken persisted state from direct StackValue projections"
        );
        assert!(
            !source.contains("BinarySerializer::serialize(&state.to_stack_item()")
                && !source.contains("BinarySerializer::serialize(&stack_item"),
            "{name} should not serialize NeoToken persisted state through local StackItem"
        );
    }
    for local_builder in [
        "StackItem::from_struct",
        "StackItem::from_array",
        "StackItem::from_int",
        "StackItem::from_bool",
        "StackItem::from_byte_string",
    ] {
        assert!(
            !types.contains(local_builder) && !native_account_projection.contains(local_builder),
            "NeoToken state projections should not hand-build stack-item shapes with \
             {local_builder}; build neo_vm::StackValue first"
        );
    }
}

#[test]
fn neo_token_committee_payloads_use_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let committee =
        read_source(workspace.join("neo-native-contracts/src/native/neo_token/committee.rs"));
    let governance =
        read_source(workspace.join("neo-native-contracts/src/native/neo_token/governance.rs"));

    assert!(
        committee.contains("BinarySerializer::deserialize_stack_value(&bytes)")
            && committee.contains("decode_committee_stack_value")
            && committee.contains("decode_committee_with_votes_value"),
        "NeoToken committee cache reads should deserialize direct neo_vm::StackValue payloads"
    );
    assert!(
        committee.contains("StackValue::Array")
            && committee.contains("StackValue::Struct")
            && committee.contains("StackValue::ByteString")
            && committee.contains("StackValue::BigInteger")
            && (committee.contains("BinarySerializer::serialize_stack_value")
                || committee.contains("serialize_stack_value_native")),
        "NeoToken committee cache writes should serialize direct StackValue arrays"
    );
    for local_path in [
        "BinarySerializer::deserialize(&bytes",
        "BinarySerializer::serialize(&array",
        "decode_committee_stack_item",
        "decode_committee_with_votes(stack_item)",
        "StackItem::from_struct",
        "StackItem::from_array",
        "StackItem::from_int",
        "StackItem::from_byte_string",
    ] {
        assert!(
            !committee.contains(local_path),
            "NeoToken committee cache should not use local StackItem path {local_path}"
        );
    }

    for method in [
        "pub(super) fn get_candidates",
        "pub(super) fn get_committee",
        "pub(super) fn get_next_block_validators",
    ] {
        let section = governance
            .split(method)
            .nth(1)
            .and_then(|tail| tail.split("\n    pub(super) fn ").next())
            .expect("NeoToken governance method section");
        assert!(
            section.contains("StackValue::Array")
                && (section.contains("BinarySerializer::serialize_stack_value")
                    || section.contains("serialize_stack_value_native(")),
            "{method} should serialize its ABI result through neo_vm::StackValue"
        );
        assert!(
            !section.contains("StackItem::from_array")
                && !section.contains("StackItem::from_struct")
                && !section.contains("StackItem::from_byte_string")
                && !section.contains("StackItem::from_int")
                && !section.contains("BinarySerializer::serialize(&array"),
            "{method} should not hand-build local StackItem result payloads"
        );
    }
}

#[test]
fn token_management_states_project_through_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let source =
        read_source(workspace.join("neo-native-contracts/src/native/token_management/state.rs"));
    let storage =
        read_source(workspace.join("neo-native-contracts/src/native/token_management/storage.rs"));
    let state_section = source.as_str();

    assert!(
        source.contains("use neo_vm::StackValue;"),
        "TokenManagement state structs should import neo_vm::StackValue directly for \
         native data projection"
    );
    assert!(
        source
            .matches("pub fn to_stack_value(&self) -> StackValue")
            .count()
            >= 3,
        "TokenState, AccountState, and NFTState should expose direct StackValue projections"
    );
    assert!(
        source
            .matches(
                "pub fn from_stack_value(&mut self, stack_value: StackValue) -> Result<(), CoreError>"
            )
            .count()
            >= 3,
        "TokenState, AccountState, and NFTState should parse direct StackValue inputs before \
         adapting local StackItem"
    );
    assert!(
        source
            .matches("stack_item_to_stack_value(stack_item")
            .count()
            >= 3,
        "TokenManagement state from_stack_item paths should adapt into StackValue before parsing"
    );
    assert!(
        source
            .matches("stack_value_to_stack_item(self.to_stack_value()")
            .count()
            >= 3,
        "TokenManagement state to_stack_item paths should adapt from direct StackValue projections"
    );
    assert!(
        storage.contains("BinarySerializer::deserialize_stack_value_with_limits"),
        "TokenManagement persisted state reads should deserialize directly into \
         neo_vm::StackValue"
    );
    assert!(
        storage.contains("BinarySerializer::serialize_stack_value"),
        "TokenManagement persisted state writes should serialize direct StackValue projections"
    );
    assert!(
        !storage.contains("BinarySerializer::deserialize("),
        "TokenManagement should not deserialize persisted state through local StackItem"
    );
    assert!(
        !storage.contains(".to_stack_item()?"),
        "TokenManagement should not bounce persisted state writes through local StackItem callers"
    );
    for local_builder in [
        "StackItem::from_struct",
        "StackItem::from_array",
        "StackItem::from_int",
        "StackItem::from_bool",
        "StackItem::from_byte_string",
    ] {
        assert!(
            !state_section.contains(local_builder),
            "TokenManagement state projections should not hand-build stack-item shapes with \
             {local_builder}; build neo_vm::StackValue first"
        );
    }
}

#[test]
fn role_management_designated_nodes_storage_uses_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let source = fs::read_to_string(
        workspace.join("neo-native-contracts/src/native/role_management/storage.rs"),
    )
    .unwrap();
    let serialization_section = source
        .split("/// Serializes public keys to bytes")
        .nth(1)
        .and_then(|tail| tail.split("/// Parses public keys from bytes").next())
        .expect("role-management public-key serialization section");
    let parsing_section = source
        .split("/// Parses public keys from bytes")
        .nth(1)
        .expect("role-management public-key parsing section");

    assert!(
        source.contains("use neo_vm::StackValue;"),
        "RoleManagement persisted designated-node payloads should import neo_vm::StackValue"
    );
    assert!(
        serialization_section.contains("StackValue::Array")
            && serialization_section.contains("StackValue::ByteString")
            && serialization_section.contains("BinarySerializer::serialize_stack_value"),
        "RoleManagement public-key storage writes should serialize direct StackValue arrays"
    );
    assert!(
        parsing_section.contains("BinarySerializer::deserialize_stack_value(data)")
            && parsing_section.contains("StackValue::Array")
            && parsing_section.contains("to_byte_string_bytes()"),
        "RoleManagement public-key storage reads should deserialize direct StackValue arrays"
    );
    assert!(
        !serialization_section.contains("StackItem::from_array")
            && !serialization_section.contains("StackItem::from_byte_string")
            && !serialization_section.contains("BinarySerializer::serialize(&array"),
        "RoleManagement public-key storage writes should not hand-build local StackItem arrays"
    );
    assert!(
        !parsing_section.contains("BinarySerializer::deserialize(data")
            && !parsing_section.contains("StackItem::Array")
            && !parsing_section.contains(".as_bytes()"),
        "RoleManagement public-key storage reads should not parse through local StackItem"
    );
}

#[test]
fn script_builder_does_not_return_local_script_objects() {
    let workspace = workspace_root();
    let script_builder = read_source(workspace.join("neo-vm/src/script_builder.rs"));
    assert!(!workspace.join("neo-core/src/script_builder.rs").exists());
    assert!(script_builder.contains("pub fn to_array(&self) -> Vec<u8>"));

    assert!(
        !script_builder.contains("use crate::neo_vm::script::Script")
            && !script_builder.contains("pub fn to_script")
            && !script_builder.contains("Script::new_relaxed"),
        "ScriptBuilder should return script bytes only; callers that need the local VM \
         runtime can construct neo_vm::Script explicitly at that boundary"
    );
}

#[test]
fn script_builder_uses_authoritative_vm_errors_without_a_core_adapter() {
    let workspace = workspace_root();
    let script_builder = read_source(workspace.join("neo-vm/src/script_builder.rs"));
    assert!(!workspace.join("neo-core/src/script_builder.rs").exists());
    assert!(script_builder.contains("use crate::error::{VmError, VmResult};"));
    assert!(
        !script_builder.contains("crate::neo_vm::error") && !script_builder.contains("neo_core")
    );
    let mut builder = ScriptBuilder::new();
    let result: neo_vm::VmResult<&mut ScriptBuilder> =
        builder.emit_push_stack_value(&StackValue::Null);
    result.unwrap();
}

#[test]
fn contract_management_uses_direct_script_validation_for_nef_abi_checks() {
    let workspace = workspace_root();
    for relative in [
        "neo-native-contracts/src/native/contract_management/deploy.rs",
        "neo-native-contracts/src/native/contract_management/update.rs",
        "neo-native-contracts/src/native/contract_management/validation.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        assert!(
            source.contains("script_validation"),
            "{relative} should validate NEF scripts and ABI offsets through the direct \
             neo-vm-rs script validator"
        );
        assert!(
            !source.contains("use crate::neo_vm::Script")
                && !source.contains("Script::new(")
                && !source.contains("script: &Script"),
            "{relative} should not depend on local neo_vm::Script for deploy/update \
             bytecode validation"
        );
    }
}

#[test]
fn neo_vm_module_docs_do_not_claim_local_canonical_opcode_semantics() {
    let workspace = workspace_root();
    let module_docs = read_source(workspace.join("neo-vm/src/lib.rs"));

    for forbidden in [
        "Complete Opcode Support",
        "Opcode implementations and dispatch",
        "contains implementations for all VM opcodes",
    ] {
        assert!(
            !module_docs.contains(forbidden),
            "neo-core::neo_vm docs must not imply local canonical NeoVM semantics: {forbidden}"
        );
    }

    assert!(
        module_docs.contains("fully self-contained")
            && module_docs.contains("canonical opcode metadata"),
        "the neo-vm crate docs should state that it is the self-contained canonical \
         home of opcode metadata and ABI-level semantics"
    );
}

#[test]
fn newbuffer_reuses_neo_vm_rs_collection_semantics() {
    let workspace = workspace_root();
    let splice = read_source(workspace.join("neo-vm/src/jump_table/splice.rs"));

    assert!(
        splice.contains("crate::semantics::collections::new_buffer"),
        "NEWBUFFER should reuse neo-vm-rs collection semantics instead of rebuilding \
         the StackValue rule locally"
    );
    assert!(
        !splice.contains("vec![0; size]"),
        "NEWBUFFER should not hand-roll zero-filled Buffer allocation"
    );
}

#[test]
fn array_and_struct_constructors_reuse_neo_vm_rs_collection_semantics() {
    let workspace = workspace_root();
    let compound = read_source(workspace.join("neo-vm/src/jump_table/compound.rs"));

    assert!(
        compound.contains("crate::semantics::collections::new_array("),
        "NEWARRAY should reuse neo-vm-rs collection semantics instead of rebuilding \
         null-filled Array values locally"
    );
    assert!(
        compound.contains("crate::semantics::collections::new_array_t"),
        "NEWARRAY_T should reuse neo-vm-rs typed-array default semantics"
    );
    assert!(
        compound.contains("crate::semantics::collections::new_struct"),
        "NEWSTRUCT should reuse neo-vm-rs collection semantics instead of rebuilding \
         null-filled Struct values locally"
    );
    assert!(
        !compound.contains("new_array_default_value_for_type_tag"),
        "NEWARRAY_T should not duplicate neo-vm-rs default-value selection locally"
    );
    assert!(
        !compound.contains("items.push(StackItem::Null)"),
        "array and struct constructors should not hand-roll null-fill loops"
    );
}

#[test]
fn newmap_reuses_neo_vm_rs_map_semantics() {
    let workspace = workspace_root();
    let compound = read_source(workspace.join("neo-vm/src/jump_table/compound.rs"));

    assert!(
        compound.contains("crate::semantics::collections::pack_map(Vec::new())"),
        "NEWMAP should reuse neo-vm-rs empty-map semantics instead of constructing the \
         map directly in the opcode handler"
    );
    assert!(
        !compound.contains(
            "let map = Map::new(BTreeMap::new(), Some(context.reference_counter().clone()))?;"
        ),
        "NEWMAP should not hand-roll empty map construction"
    );
}

#[test]
fn stack_item_byte_conversion_reuses_neo_vm_rs_stack_value_rules() {
    let workspace = workspace_root();
    let stack_item = read_source(workspace.join("neo-vm/src/stack_item/stack_item.rs"));

    assert!(
        stack_item.contains("to_byte_string_bytes()"),
        "StackItem byte conversion should reuse neo-vm-rs StackValue byte-string rules"
    );
    assert!(
        !stack_item.contains("fn normalize_bigint_bytes"),
        "StackItem should not keep a local BigInt byte normalization helper"
    );
    assert!(
        !stack_item.contains("Self::Boolean(b) => Ok(vec![u8::from(*b)])"),
        "StackItem::as_bytes should not duplicate primitive byte conversion locally"
    );
    assert!(
        !stack_item.contains("Self::Boolean(b) => Ok(vec![u8::from(b)])"),
        "StackItem::into_bytes should not duplicate primitive byte conversion locally"
    );
}

#[test]
fn stack_item_primitive_truthiness_reuses_neo_vm_rs_rules() {
    let workspace = workspace_root();
    let stack_item = read_source(workspace.join("neo-vm/src/stack_item/stack_item.rs"));

    assert!(
        stack_item.contains("crate::semantics::comparison::boolean_value"),
        "StackItem primitive truthiness should reuse neo-vm-rs StackValue truthiness rules"
    );
    assert!(
        stack_item.contains("StackValue::ByteString(b.clone())"),
        "ByteString truthiness should adapt through neo-vm-rs after preserving the local max-size guard"
    );
    assert!(
        !stack_item.contains("Self::Integer(i) => Ok(!i.is_zero())"),
        "StackItem::as_bool should not keep local integer truthiness"
    );
    assert!(
        !stack_item.contains("Ok(b.iter().any(|&byte| byte != 0))"),
        "StackItem::as_bool should not keep local ByteString truthiness after the max-size guard"
    );
}

#[test]
fn stack_item_convert_to_byte_targets_reuses_neo_vm_rs_conversion_semantics() {
    let workspace = workspace_root();
    let stack_item = read_source(workspace.join("neo-vm/src/stack_item/stack_item.rs"));

    assert!(
        stack_item.contains("crate::semantics::conversion::convert_value"),
        "StackItem::convert_to should reuse neo-vm-rs conversion semantics for primitive \
         ByteString/Buffer targets"
    );
    assert!(
        stack_item.contains("target_type @ (StackItemType::ByteString | StackItemType::Buffer)"),
        "StackItem::convert_to should restrict neo-vm-rs byte-target conversion reuse to \
         ByteString/Buffer targets"
    );
    assert!(
        !stack_item.contains("StackItemType::ByteString => Ok(Self::ByteString(self.as_bytes()?))"),
        "StackItem::convert_to should not keep a broad local ByteString conversion branch"
    );
    assert!(
        !stack_item.contains(
            "StackItemType::Buffer => Ok(Self::Buffer(BufferItem::new(self.as_bytes()?)))"
        ),
        "StackItem::convert_to should not keep a broad local Buffer conversion branch"
    );
}

#[test]
fn stack_item_convert_to_boolean_reuses_neo_vm_rs_for_safe_sources_only() {
    let workspace = workspace_root();
    let stack_item = read_source(workspace.join("neo-vm/src/stack_item/stack_item.rs"));

    assert!(
        stack_item.contains("target_type @ StackItemType::Boolean"),
        "StackItem::convert_to should route safe Boolean conversions through neo-vm-rs \
         conversion semantics"
    );
    assert!(
        stack_item.contains("Cannot convert ByteString to Boolean"),
        "StackItem::convert_to(Boolean) must keep the local over-sized ByteString guard \
         before delegating to neo-vm-rs"
    );
    assert!(
        stack_item.contains("StackItemType::Boolean => Ok(Self::Boolean(self.as_bool()?))"),
        "StackItem::convert_to(Boolean) must keep the local fallback for Buffer and \
         compound truthiness, whose semantics differ from neo-vm-rs"
    );
}

#[test]
fn size_primitive_lengths_reuse_neo_vm_rs_byte_string_rules() {
    let workspace = workspace_root();
    let compound = read_source(workspace.join("neo-vm/src/jump_table/compound.rs"));

    assert!(
        compound.contains("crate::semantics::collections::size(&value)"),
        "SIZE for primitive Integer/Boolean values should reuse neo-vm-rs collection \
         semantics"
    );
    assert!(
        !compound.contains("minimal two's-complement encoded byte count"),
        "SIZE should not keep a local integer byte-count implementation"
    );
    assert!(
        !compound.contains("bi.to_signed_bytes_le()"),
        "SIZE should not duplicate BigInt byte encoding locally"
    );
}

#[test]
fn boolean_numeric_opcodes_reuse_neo_vm_rs_comparison_semantics() {
    let workspace = workspace_root();
    let numeric = read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

    assert!(
        numeric.contains("comparison::not_value(&value)"),
        "NOT should reuse neo-vm-rs strict boolean comparison semantics through StackValue"
    );
    assert!(
        numeric.contains("comparison::bool_and(left, right)"),
        "BOOLAND should reuse neo-vm-rs boolean comparison semantics after local truthiness conversion"
    );
    assert!(
        numeric.contains("comparison::bool_or(left, right)"),
        "BOOLOR should reuse neo-vm-rs boolean comparison semantics after local truthiness conversion"
    );
    assert!(
        !numeric.contains("StackItem::from_bool(!x)"),
        "NOT should not keep a local boolean negation rule"
    );
    assert!(
        !numeric.contains("StackItem::from_bool(a && b)"),
        "BOOLAND should not keep a local boolean conjunction rule"
    );
    assert!(
        !numeric.contains("StackItem::from_bool(a || b)"),
        "BOOLOR should not keep a local boolean disjunction rule"
    );
}

#[test]
fn nz_reuses_neo_vm_rs_truthiness_semantics() {
    let workspace = workspace_root();
    let numeric = read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

    assert!(
        numeric.contains("comparison::nz_value(&value)"),
        "NZ should reuse neo-vm-rs numeric nonzero semantics through StackValue"
    );
    assert!(
        numeric.contains("let value = value_from_stack_item(ctx.pop()?)?;"),
        "NZ should adapt the local stack item into a neo-vm-rs StackValue before evaluation"
    );
    assert!(
        !numeric.contains("StackItem::from_bool(!value.is_zero())"),
        "NZ should not keep a local nonzero predicate"
    );
}

#[test]
fn sign_reuses_neo_vm_rs_i64_semantics_with_bigint_fallback() {
    let workspace = workspace_root();
    let numeric = read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

    assert!(
        numeric.contains("unary_numeric(engine, arithmetic::sign_value)"),
        "SIGN should reuse neo-vm-rs StackValue arithmetic sign semantics"
    );
    assert!(
        numeric.contains("fn value_from_stack_item(item: StackItem) -> VmResult<StackValue>"),
        "SIGN should adapt local VM values at the StackValue boundary"
    );
    assert!(
        !numeric.contains("value.sign()"),
        "SIGN should not keep a local BigInt sign fallback after neo-vm-rs owns BigInt \
         StackValue semantics"
    );

    assert_eq!(execute_sign(BigInt::from(i64::MIN)), BigInt::from(-1));
    assert_eq!(execute_sign(BigInt::from(0)), BigInt::from(0));
    assert_eq!(execute_sign(BigInt::from(i64::MAX)), BigInt::from(1));
    let wide_positive: BigInt = BigInt::from(1u128) << 100usize;
    let wide_negative = -wide_positive.clone();
    assert_eq!(execute_sign(wide_positive), BigInt::from(1));
    assert_eq!(execute_sign(wide_negative), BigInt::from(-1));
}

#[test]
fn sqrt_reuses_neo_vm_rs_i64_semantics_with_bigint_fallback() {
    let workspace = workspace_root();
    let numeric = read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

    assert!(
        numeric.contains("unary_numeric(engine, arithmetic::sqrt_value)"),
        "SQRT should reuse neo-vm-rs StackValue arithmetic sqrt semantics"
    );
    assert!(
        numeric.contains("fn value_from_stack_item(item: StackItem) -> VmResult<StackValue>"),
        "SQRT should adapt local VM values at the StackValue boundary"
    );
    assert!(
        !numeric.contains("value.is_negative()"),
        "SQRT should not keep a local negative-value guard after neo-vm-rs owns the rule"
    );
    assert!(
        !numeric.contains("integer_sqrt(&value)"),
        "SQRT should not keep a local BigInt fallback after neo-vm-rs owns BigInt \
         StackValue semantics"
    );

    assert_eq!(
        execute_unary_int(OpCode::SQRT, BigInt::from(144)),
        BigInt::from(12)
    );

    let wide_root: BigInt = BigInt::from(1u128) << 100usize;
    let wide_square = &wide_root * &wide_root;
    assert_eq!(execute_unary_int(OpCode::SQRT, wide_square), wide_root);
}

#[test]
fn checked_unary_arithmetic_reuses_neo_vm_rs_i64_semantics_with_bigint_fallback() {
    let workspace = workspace_root();
    let numeric = read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

    for helper in ["inc_value", "dec_value", "negate_value", "abs_value"] {
        assert!(
            numeric.contains(&format!("unary_numeric(engine, arithmetic::{helper})")),
            "checked unary arithmetic should reuse neo-vm-rs StackValue helper {helper}"
        );
    }
    for guard in ["checked_add", "checked_sub", "checked_neg", "checked_abs"] {
        assert!(
            !numeric.contains(guard),
            "checked unary arithmetic should not keep local checked {guard} fallbacks after \
             neo-vm-rs owns BigInt StackValue semantics"
        );
    }
    assert!(
        !numeric.contains("check_bigint_size(&result)?"),
        "checked unary arithmetic should not keep local BigInt fallback sizing after \
         neo-vm-rs owns StackValue integer result sizing"
    );

    assert_eq!(
        execute_unary_int(OpCode::INC, BigInt::from(41)),
        BigInt::from(42)
    );
    assert_eq!(
        execute_unary_int(OpCode::DEC, BigInt::from(41)),
        BigInt::from(40)
    );
    assert_eq!(
        execute_unary_int(OpCode::NEGATE, BigInt::from(41)),
        BigInt::from(-41)
    );
    assert_eq!(
        execute_unary_int(OpCode::ABS, BigInt::from(-41)),
        BigInt::from(41)
    );

    assert_eq!(
        execute_unary_int(OpCode::INC, BigInt::from(i64::MAX)),
        BigInt::from(i64::MAX) + 1
    );
    assert_eq!(
        execute_unary_int(OpCode::DEC, BigInt::from(i64::MIN)),
        BigInt::from(i64::MIN) - 1
    );
    assert_eq!(
        execute_unary_int(OpCode::NEGATE, BigInt::from(i64::MIN)),
        -(BigInt::from(i64::MIN))
    );
    assert_eq!(
        execute_unary_int(OpCode::ABS, BigInt::from(i64::MIN)),
        -(BigInt::from(i64::MIN))
    );

    let wide: BigInt = BigInt::from(1u128) << 100usize;
    assert_eq!(
        execute_unary_int(OpCode::INC, wide.clone()),
        wide.clone() + 1
    );
    assert_eq!(
        execute_unary_int(OpCode::DEC, wide.clone()),
        wide.clone() - 1
    );
    assert_eq!(
        execute_unary_int(OpCode::NEGATE, wide.clone()),
        -wide.clone()
    );
    assert_eq!(execute_unary_int(OpCode::ABS, -wide.clone()), wide);
}

#[test]
fn checked_binary_arithmetic_reuses_neo_vm_rs_i64_semantics_with_bigint_fallback() {
    let workspace = workspace_root();
    let numeric = read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

    for helper in [
        "add_values",
        "sub_values",
        "mul_values",
        "div_values",
        "modulo_values",
    ] {
        assert!(
            numeric.contains(&format!("binary_numeric(engine, arithmetic::{helper})")),
            "checked binary arithmetic should reuse neo-vm-rs StackValue helper {helper}"
        );
    }
    for guard in [
        "checked_add",
        "checked_sub",
        "checked_mul",
        "checked_div",
        "checked_rem",
    ] {
        assert!(
            !numeric.contains(guard),
            "checked binary arithmetic should not keep local checked {guard} fallbacks after \
             neo-vm-rs owns BigInt StackValue semantics"
        );
    }
    assert!(
        !numeric.contains("check_bigint_size(&result)?"),
        "checked binary arithmetic should not keep local BigInt fallback sizing after \
         neo-vm-rs owns StackValue integer result sizing"
    );

    assert_eq!(
        execute_binary_int(OpCode::ADD, BigInt::from(20), BigInt::from(22)),
        BigInt::from(42)
    );
    assert_eq!(
        execute_binary_int(OpCode::SUB, BigInt::from(44), BigInt::from(2)),
        BigInt::from(42)
    );
    assert_eq!(
        execute_binary_int(OpCode::MUL, BigInt::from(6), BigInt::from(7)),
        BigInt::from(42)
    );
    assert_eq!(
        execute_binary_int(OpCode::DIV, BigInt::from(84), BigInt::from(2)),
        BigInt::from(42)
    );
    assert_eq!(
        execute_binary_int(OpCode::MOD, BigInt::from(85), BigInt::from(43)),
        BigInt::from(42)
    );

    assert_eq!(
        execute_binary_int(OpCode::ADD, BigInt::from(i64::MAX), BigInt::from(1)),
        BigInt::from(i64::MAX) + 1
    );
    assert_eq!(
        execute_binary_int(OpCode::SUB, BigInt::from(i64::MIN), BigInt::from(1)),
        BigInt::from(i64::MIN) - 1
    );
    assert_eq!(
        execute_binary_int(OpCode::MUL, BigInt::from(i64::MAX), BigInt::from(2)),
        BigInt::from(i64::MAX) * 2
    );
    assert_eq!(
        execute_binary_int(OpCode::DIV, BigInt::from(i64::MIN), BigInt::from(-1)),
        -(BigInt::from(i64::MIN))
    );
    assert_eq!(
        execute_binary_int(OpCode::MOD, BigInt::from(i64::MIN), BigInt::from(-1)),
        BigInt::from(0)
    );

    let wide: BigInt = BigInt::from(1u128) << 100usize;
    assert_eq!(
        execute_binary_int(OpCode::ADD, wide.clone(), BigInt::from(7)),
        wide.clone() + 7
    );
    assert_eq!(
        execute_binary_int(OpCode::SUB, wide.clone(), BigInt::from(7)),
        wide.clone() - 7
    );
    assert_eq!(
        execute_binary_int(OpCode::MUL, wide.clone(), BigInt::from(7)),
        wide.clone() * 7
    );
    assert_eq!(
        execute_binary_int(OpCode::DIV, wide.clone(), BigInt::from(7)),
        wide.clone() / 7
    );
    assert_eq!(
        execute_binary_int(OpCode::MOD, wide.clone(), BigInt::from(7)),
        wide % 7
    );
}

#[test]
fn pow_and_modmul_reuse_neo_vm_rs_i64_semantics_with_bigint_fallback() {
    let workspace = workspace_root();
    let numeric = read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

    for helper in ["pow_values", "modmul_values"] {
        assert!(
            numeric.contains(&format!("arithmetic::{helper}")),
            "POW/MODMUL should reuse neo-vm-rs StackValue helper {helper}"
        );
    }
    assert!(
        numeric.contains(".assert_shift(exponent_i32)"),
        "POW must keep the local execution-limit guard before delegating to neo-vm-rs"
    );
    assert!(
        !numeric.contains("a.pow(exponent_i32 as u32)") && !numeric.contains("(a * b) % modulus"),
        "POW/MODMUL should not keep local BigInt fallbacks after neo-vm-rs owns \
         StackValue integer semantics"
    );

    assert_eq!(
        execute_binary_int(OpCode::POW, BigInt::from(6), BigInt::from(2)),
        BigInt::from(36)
    );
    assert_eq!(
        execute_binary_int(OpCode::POW, BigInt::from(i64::MAX), BigInt::from(2)),
        BigInt::from(i64::MAX).pow(2)
    );

    assert_eq!(
        execute_ternary_int(
            OpCode::MODMUL,
            BigInt::from(6),
            BigInt::from(7),
            BigInt::from(100)
        ),
        BigInt::from(42)
    );

    let wide: BigInt = BigInt::from(1u128) << 100usize;
    assert_eq!(
        execute_binary_int(OpCode::POW, wide.clone(), BigInt::from(2)),
        wide.clone().pow(2)
    );
    assert_eq!(
        execute_ternary_int(
            OpCode::MODMUL,
            wide.clone(),
            BigInt::from(7),
            BigInt::from(97)
        ),
        (wide * 7) % 97
    );
}

#[test]
fn modpow_reuses_neo_vm_rs_i64_semantics_with_bigint_and_inverse_fallback() {
    let workspace = workspace_root();
    let numeric = read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

    assert!(
        numeric.contains("ternary_numeric(engine, arithmetic::modpow_values)"),
        "MODPOW should reuse neo-vm-rs StackValue modular exponentiation semantics"
    );
    assert!(
        numeric.contains("fn ternary_numeric("),
        "MODPOW should delegate through the shared StackValue ternary adapter"
    );
    assert!(
        !numeric.contains("modulus.is_positive()"),
        "MODPOW should not keep local modulus sign handling after neo-vm-rs owns the rule"
    );
    assert!(
        !numeric.contains("mod_inverse(&base, &modulus)?") && !numeric.contains("base.modpow"),
        "MODPOW should not keep local modular-inverse or BigInt fallback logic"
    );

    assert_eq!(
        execute_ternary_int(
            OpCode::MODPOW,
            BigInt::from(4),
            BigInt::from(13),
            BigInt::from(497)
        ),
        BigInt::from(445)
    );

    let wide: BigInt = BigInt::from(1u128) << 100usize;
    assert_eq!(
        execute_ternary_int(
            OpCode::MODPOW,
            wide.clone(),
            BigInt::from(3),
            BigInt::from(97)
        ),
        wide.modpow(&BigInt::from(3), &BigInt::from(97))
    );

    assert_eq!(
        execute_ternary_int(
            OpCode::MODPOW,
            BigInt::from(3),
            BigInt::from(-1),
            BigInt::from(11)
        ),
        BigInt::from(4)
    );
}

#[test]
fn shift_opcodes_reuse_neo_vm_rs_i64_semantics_with_bigint_fallback() {
    let workspace = workspace_root();
    let numeric = read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

    for helper in ["shl_value", "shr_value"] {
        // Both hardfork semantics route through the shared arithmetic
        // helpers: Gorgon (`shl_value`/`shr_value`) and pre-Gorgon
        // (`*_pre_gorgon`) selected per `zero_shift_converts_to_integer`.
        assert!(
            numeric.contains(&format!("arithmetic::{helper}"))
                && numeric.contains(&format!("arithmetic::{helper}_pre_gorgon")),
            "SHL/SHR should reuse the neo-vm-rs StackValue helpers for {helper} \
             across both hardfork semantics"
        );
    }
    assert!(
        numeric.contains(".assert_shift(shift_i32)"),
        "SHL/SHR should keep local execution-limit validation before delegating"
    );
    assert!(
        !numeric.contains("shift_i32 < 64"),
        "SHL/SHR should not keep the old i64-helper-specific shift range branch"
    );
    assert!(
        !numeric.contains("a << (shift_i32 as u32)")
            && !numeric.contains("a >> (shift_i32 as u32)"),
        "SHL/SHR should not keep local BigInt fallbacks after neo-vm-rs owns \
         StackValue integer semantics"
    );

    assert_eq!(
        execute_binary_int(OpCode::SHL, BigInt::from(21), BigInt::from(1)),
        BigInt::from(42)
    );
    assert_eq!(
        execute_binary_int(OpCode::SHR, BigInt::from(84), BigInt::from(1)),
        BigInt::from(42)
    );
    assert_eq!(
        execute_binary_int(OpCode::SHL, BigInt::from(i64::MAX), BigInt::from(1)),
        BigInt::from(i64::MAX) << 1u32
    );
    assert_eq!(
        execute_binary_int(OpCode::SHR, BigInt::from(-84), BigInt::from(1)),
        BigInt::from(-42)
    );

    let wide: BigInt = BigInt::from(1u128) << 100usize;
    assert_eq!(
        execute_binary_int(OpCode::SHL, wide.clone(), BigInt::from(1)),
        wide.clone() << 1u32
    );
    assert_eq!(
        execute_binary_int(OpCode::SHR, wide.clone(), BigInt::from(1)),
        wide >> 1u32
    );
}

#[test]
fn min_max_reuse_neo_vm_rs_i64_semantics_with_bigint_fallback() {
    let workspace = workspace_root();
    let numeric = read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

    assert!(
        numeric.contains("binary_numeric(engine, arithmetic::min_values)"),
        "MIN should reuse neo-vm-rs StackValue arithmetic min semantics"
    );
    assert!(
        numeric.contains("binary_numeric(engine, arithmetic::max_values)"),
        "MAX should reuse neo-vm-rs StackValue arithmetic max semantics"
    );
    assert!(
        numeric.contains("fn binary_numeric("),
        "MIN/MAX should delegate through the shared StackValue binary adapter"
    );
    assert!(
        !numeric.contains("if a < b { a } else { b }")
            && !numeric.contains("if a > b { a } else { b }"),
        "MIN/MAX should not keep local BigInt fallbacks after neo-vm-rs owns \
         StackValue integer semantics"
    );

    assert_eq!(
        execute_binary_int(OpCode::MIN, BigInt::from(i64::MIN), BigInt::from(7)),
        BigInt::from(i64::MIN)
    );
    assert_eq!(
        execute_binary_int(OpCode::MAX, BigInt::from(i64::MIN), BigInt::from(7)),
        BigInt::from(7)
    );

    let wide_positive: BigInt = BigInt::from(1u128) << 100usize;
    let wide_negative = -wide_positive.clone();
    assert_eq!(
        execute_binary_int(OpCode::MIN, wide_positive.clone(), BigInt::from(7)),
        BigInt::from(7)
    );
    assert_eq!(
        execute_binary_int(OpCode::MAX, wide_positive.clone(), BigInt::from(7)),
        wide_positive
    );
    assert_eq!(
        execute_binary_int(OpCode::MIN, wide_negative.clone(), BigInt::from(-7)),
        wide_negative
    );
    assert_eq!(
        execute_binary_int(
            OpCode::MAX,
            -(BigInt::from(1u128) << 100usize),
            BigInt::from(-7)
        ),
        BigInt::from(-7)
    );
}

#[test]
fn within_reuses_neo_vm_rs_i64_semantics_with_bigint_fallback() {
    let workspace = workspace_root();
    let numeric = read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

    assert!(
        numeric.contains("arithmetic::within_values(value, lower, upper)"),
        "WITHIN should reuse neo-vm-rs StackValue range semantics"
    );
    assert!(
        numeric.contains("let value = value_from_stack_item(ctx.pop()?)?;"),
        "WITHIN should adapt local VM values at the StackValue boundary"
    );
    assert!(
        !numeric.contains("a <= x && x < b"),
        "WITHIN should not keep a local BigInt fallback after neo-vm-rs owns \
         StackValue integer semantics"
    );

    assert!(execute_within(
        BigInt::from(5),
        BigInt::from(1),
        BigInt::from(10)
    ));
    assert!(!execute_within(
        BigInt::from(10),
        BigInt::from(1),
        BigInt::from(10)
    ));

    let wide_lower: BigInt = BigInt::from(1u128) << 100usize;
    let wide_value = wide_lower.clone() + BigInt::from(7);
    let wide_upper = wide_lower.clone() + BigInt::from(10);
    assert!(execute_within(
        wide_value.clone(),
        wide_lower.clone(),
        wide_upper.clone()
    ));
    assert!(!execute_within(wide_upper, wide_lower, wide_value));
}

#[test]
fn numeric_comparisons_reuse_neo_vm_rs_i64_semantics_with_bigint_fallback() {
    let workspace = workspace_root();
    let numeric = read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

    for helper in [
        "less_than_values",
        "less_or_equal_values",
        "greater_than_values",
        "greater_or_equal_values",
        "num_equal_values",
        "num_not_equal_values",
    ] {
        assert!(
            numeric.contains(&format!("comparison::{helper}")),
            "numeric comparisons should reuse neo-vm-rs StackValue helper {helper}"
        );
    }
    assert!(
        numeric.contains("fn compare_with_null(")
            && numeric.contains("fn numeric_equality(")
            && numeric.contains("let left = value_from_stack_item(left)?;"),
        "numeric comparisons should preserve local null policy and otherwise adapt through \
         StackValue"
    );
    assert!(
        !numeric.contains("cmp(&a_int, &b_int)")
            && !numeric.contains("a_int == b_int")
            && !numeric.contains("a_int != b_int"),
        "numeric comparisons should not keep local BigInt fallback comparisons after \
         neo-vm-rs owns StackValue integer semantics"
    );

    assert!(execute_binary_bool(
        OpCode::LT,
        BigInt::from(41),
        BigInt::from(42)
    ));
    assert!(execute_binary_bool(
        OpCode::LE,
        BigInt::from(42),
        BigInt::from(42)
    ));
    assert!(execute_binary_bool(
        OpCode::GT,
        BigInt::from(43),
        BigInt::from(42)
    ));
    assert!(execute_binary_bool(
        OpCode::GE,
        BigInt::from(42),
        BigInt::from(42)
    ));
    assert!(execute_binary_bool(
        OpCode::NUMEQUAL,
        BigInt::from(42),
        BigInt::from(42)
    ));
    assert!(execute_binary_bool(
        OpCode::NUMNOTEQUAL,
        BigInt::from(41),
        BigInt::from(42)
    ));

    let wide: BigInt = BigInt::from(1u128) << 100usize;
    assert!(execute_binary_bool(
        OpCode::LT,
        wide.clone(),
        wide.clone() + 1
    ));
    assert!(execute_binary_bool(OpCode::LE, wide.clone(), wide.clone()));
    assert!(execute_binary_bool(
        OpCode::GT,
        wide.clone() + 1,
        wide.clone()
    ));
    assert!(execute_binary_bool(OpCode::GE, wide.clone(), wide.clone()));
    assert!(execute_binary_bool(
        OpCode::NUMEQUAL,
        wide.clone(),
        wide.clone()
    ));
    assert!(execute_binary_bool(
        OpCode::NUMNOTEQUAL,
        wide.clone(),
        wide + 1
    ));
}

#[test]
fn bitwise_opcodes_reuse_neo_vm_rs_i64_semantics_with_bigint_fallback() {
    let workspace = workspace_root();
    let bitwise = read_source(workspace.join("neo-vm/src/jump_table/bitwisee.rs"));

    for helper in [
        "invert_value",
        "bitwise_and_values",
        "bitwise_or_values",
        "bitwise_xor_values",
    ] {
        assert!(
            bitwise.contains(&format!("arithmetic::{helper}")),
            "bitwise opcode handlers should reuse neo-vm-rs StackValue helper {helper}"
        );
    }
    assert!(
        bitwise.contains("fn value_from_stack_item(item: StackItem) -> VmResult<StackValue>")
            && bitwise.contains("fn binary_bitwise("),
        "bitwise opcode handlers should adapt local operands through StackValue"
    );
    assert!(
        !bitwise.contains("StackItem::from_int(!x)")
            && !bitwise.contains("StackItem::from_int(bigint_op(a, b))"),
        "bitwise opcode handlers should not keep local BigInt fallbacks after neo-vm-rs \
         owns StackValue integer semantics"
    );

    assert_eq!(
        execute_unary_int(OpCode::INVERT, BigInt::from(0)),
        BigInt::from(-1)
    );
    assert_eq!(
        execute_binary_int(OpCode::AND, BigInt::from(-1), BigInt::from(0x55)),
        BigInt::from(0x55)
    );
    assert_eq!(
        execute_binary_int(OpCode::OR, BigInt::from(0x50), BigInt::from(0x05)),
        BigInt::from(0x55)
    );
    assert_eq!(
        execute_binary_int(OpCode::XOR, BigInt::from(0x5a), BigInt::from(0x0f)),
        BigInt::from(0x55)
    );

    let wide: BigInt = BigInt::from(1u128) << 100usize;
    assert_eq!(
        execute_unary_int(OpCode::INVERT, wide.clone()),
        !wide.clone()
    );
    assert_eq!(
        execute_binary_int(OpCode::AND, wide.clone(), BigInt::from(-1)),
        wide.clone()
    );
    assert_eq!(
        execute_binary_int(OpCode::OR, wide.clone(), BigInt::from(0x55)),
        wide.clone() | BigInt::from(0x55)
    );
    assert_eq!(
        execute_binary_int(OpCode::XOR, wide.clone(), BigInt::from(0x55)),
        wide ^ BigInt::from(0x55)
    );
}

#[test]
fn isnull_reuses_neo_vm_rs_null_predicate() {
    let workspace = workspace_root();
    let types = read_source(workspace.join("neo-vm/src/jump_table/types.rs"));

    assert!(
        types.contains("crate::semantics::comparison::is_null"),
        "ISNULL should reuse neo-vm-rs null predicate through a narrow local StackItem adapter"
    );
    assert!(
        types.contains("StackValue::Null"),
        "ISNULL should adapt local StackItem::Null to neo-vm-rs StackValue::Null"
    );
    assert!(
        !types.contains("let result = matches!(item, StackItem::Null)"),
        "ISNULL should not keep its local null predicate after adapting to neo-vm-rs"
    );
}

#[test]
fn istype_reuses_neo_vm_rs_type_semantics_with_shallow_adapter() {
    let workspace = workspace_root();
    let types = read_source(workspace.join("neo-vm/src/jump_table/types.rs"));

    assert!(
        types.contains("crate::semantics::conversion::is_type"),
        "ISTYPE should reuse neo-vm-rs type predicate semantics"
    );
    assert!(
        types.contains("fn stack_item_type_probe_value"),
        "ISTYPE should adapt local StackItem values through a shallow type probe, not a \
         full StackValue conversion"
    );
    assert!(
        types.contains("StackItemType::Integer => StackValue::Integer(0)"),
        "ISTYPE's type probe should classify all local integers as neo-vm-rs Integer, \
         including wide VmInteger values"
    );
    assert!(
        types.contains("StackItemType::Pointer => StackValue::Pointer(0)")
            && types.contains("StackItemType::InteropInterface => StackValue::Interop(0)"),
        "ISTYPE's type probe should preserve pointer and interop type tags without \
         requiring runtime identity"
    );
    assert!(
        !types.contains("let result = item.stack_item_type() == item_type;"),
        "ISTYPE should not keep a local type equality predicate"
    );
}

#[test]
fn convert_byte_sequence_targets_reuse_neo_vm_rs_conversion_semantics() {
    let workspace = workspace_root();
    let types = read_source(workspace.join("neo-vm/src/jump_table/types.rs"));
    let stack_item = read_source(workspace.join("neo-vm/src/stack_item/stack_item.rs"));

    assert!(
        stack_item.contains("crate::semantics::conversion::convert_value"),
        "CONVERT to ByteString/Buffer should reuse neo-vm-rs conversion semantics \
         through StackItem::convert_to after local null and same-type fast paths"
    );
    assert!(
        stack_item.contains("StackItemType::ByteString | StackItemType::Buffer"),
        "CONVERT should restrict neo-vm-rs conversion reuse to ByteString/Buffer targets"
    );
    assert!(
        types.contains("item.convert_to(target_type)?"),
        "CONVERT opcode should delegate primitive conversion branches to StackItem::convert_to"
    );
    assert!(
        !types.contains(
            "(item, StackItemType::ByteString) => StackItem::from_byte_string(item.into_bytes()?)"
        ),
        "CONVERT to ByteString should not keep a local byte conversion branch"
    );
    assert!(
        !types.contains(
            "(item, StackItemType::Buffer) => StackItem::from_buffer(item.into_bytes()?)"
        ),
        "CONVERT to Buffer should not keep a local byte conversion branch"
    );
}

#[test]
fn convert_opcode_primitive_targets_reuse_stack_item_conversion_boundary() {
    let workspace = workspace_root();
    let types = read_source(workspace.join("neo-vm/src/jump_table/types.rs"));

    assert!(
        types.contains(
            "target_type @ (StackItemType::Boolean\n                | StackItemType::Integer"
        ),
        "CONVERT opcode should group primitive target types instead of duplicating StackItem \
         conversion semantics"
    );
    assert!(
        !types.contains("(item, StackItemType::Boolean) => StackItem::from_bool(item.as_bool()?)"),
        "CONVERT opcode should not duplicate boolean conversion"
    );
    assert!(
        !types.contains("(item, StackItemType::Integer) => StackItem::from_int(item.into_int()?)"),
        "CONVERT opcode should not duplicate integer conversion"
    );
    assert!(
        !types.contains("fn convert_byte_sequence_target_with_neo_vm_rs"),
        "CONVERT opcode should not keep a second byte-target neo-vm-rs adapter now that \
         StackItem::convert_to owns that boundary"
    );
}

#[test]
fn memcpy_source_reuses_neo_vm_rs_byte_sequence_semantics() {
    let workspace = workspace_root();
    let splice = read_source(workspace.join("neo-vm/src/jump_table/splice.rs"));

    assert!(
        splice.contains("splice_rules::memcpy_bytes"),
        "MEMCPY source classification and bounds should reuse neo-vm-rs splice semantics"
    );
    assert!(
        !splice.contains("StackItem::ByteString(data) => std::borrow::Cow::Borrowed"),
        "MEMCPY should not keep a local ByteString/Buffer source-type table"
    );
}

#[test]
fn splice_byte_sequence_ops_reuse_neo_vm_rs_helpers() {
    let workspace = workspace_root();
    let splice = read_source(workspace.join("neo-vm/src/jump_table/splice.rs"));

    assert!(
        splice.contains("splice_rules::cat_values"),
        "CAT should reuse neo-vm-rs opcode-level splice concatenation"
    );
    assert!(
        splice.contains("splice_rules::substr_value")
            && splice.contains("splice_rules::left_value")
            && splice.contains("splice_rules::right_value"),
        "SUBSTR, LEFT, and RIGHT should reuse neo-vm-rs opcode-level splice slicing"
    );
    assert!(
        !splice.contains("result.extend_from_slice(&x1);")
            && !splice.contains("result.extend_from_slice(&x2);"),
        "CAT should not hand-roll byte concatenation after adapting operands to neo-vm-rs"
    );
    assert!(
        !splice.contains("data[offset..offset + count].to_vec()")
            && !splice.contains("data[..count].to_vec()")
            && !splice.contains("data[data.len() - count..].to_vec()"),
        "SUBSTR, LEFT, and RIGHT should not hand-roll byte slicing after adapting operands \
         to neo-vm-rs"
    );

    let mut cat = ScriptBuilder::new();
    cat.emit_push_int(128);
    cat.emit_push_bool(true);
    cat.emit_opcode(OpCode::CAT);
    cat.emit_opcode(OpCode::RET);
    assert_eq!(execute_bytes(cat), vec![0x80, 0x00, 0x01]);

    let mut substr = ScriptBuilder::new();
    substr.emit_push_byte_array(b"abcdef");
    substr.emit_push_int(2);
    substr.emit_push_int(3);
    substr.emit_opcode(OpCode::SUBSTR);
    substr.emit_opcode(OpCode::RET);
    assert_eq!(execute_bytes(substr), b"cde".to_vec());

    let mut left = ScriptBuilder::new();
    left.emit_push_byte_array(b"abcdef");
    left.emit_push_int(2);
    left.emit_opcode(OpCode::LEFT);
    left.emit_opcode(OpCode::RET);
    assert_eq!(execute_bytes(left), b"ab".to_vec());

    let mut right = ScriptBuilder::new();
    right.emit_push_byte_array(b"abcdef");
    right.emit_push_int(2);
    right.emit_opcode(OpCode::RIGHT);
    right.emit_opcode(OpCode::RET);
    assert_eq!(execute_bytes(right), b"ef".to_vec());
}

#[test]
fn compound_byte_sequence_reads_reuse_neo_vm_rs_collection_semantics() {
    let workspace = workspace_root();
    let compound = read_source(workspace.join("neo-vm/src/jump_table/compound.rs"));

    assert!(
        compound.contains("crate::semantics::collections::has_key"),
        "HASKEY for byte sequences should reuse neo-vm-rs collection semantics after \
         local index adaptation"
    );
    assert!(
        compound.contains("crate::semantics::collections::pick_item"),
        "PICKITEM for byte sequences should reuse neo-vm-rs collection semantics after \
         local index validation"
    );
    assert!(
        compound.contains("StackValue::ByteString(bytes.clone())")
            && compound.contains("StackValue::Buffer(buffer.data())"),
        "byte-sequence collection adapters should preserve local ByteString/Buffer data"
    );
    assert!(
        !compound.contains("index >= 0 && (index as usize) < data.len()"),
        "HASKEY byte-sequence branches should not keep local byte index checks"
    );

    let mut has_key = ScriptBuilder::new();
    has_key.emit_push_byte_array(b"abc");
    has_key.emit_push_int(1);
    has_key.emit_opcode(OpCode::HASKEY);
    has_key.emit_opcode(OpCode::RET);
    assert!(execute_bool(has_key));

    let mut pick_item = ScriptBuilder::new();
    pick_item.emit_push_byte_array(b"abc");
    pick_item.emit_push_int(1);
    pick_item.emit_opcode(OpCode::PICKITEM);
    pick_item.emit_opcode(OpCode::RET);
    assert_eq!(execute_int(pick_item), BigInt::from(b'b'));
}

#[test]
fn neo_rpc_uses_neo_vm_rs_opcode_directly() {
    let workspace = workspace_root();
    let rpc_manifest = fs::read_to_string(workspace.join("neo-rpc/Cargo.toml")).unwrap();
    assert!(
        rpc_manifest.contains("neo-vm = { workspace = true"),
        "neo-rpc should depend on the local neo-vm crate when it needs OpCode"
    );

    let rpc_dir = workspace.join("neo-rpc/src");
    let mut offenders = Vec::new();
    collect_rs_files(&rpc_dir, &mut offenders);
    offenders.retain(|path| {
        fs::read_to_string(path)
            .is_ok_and(|source| source.contains("neo_core::neo_vm::op_code::OpCode"))
    });

    assert!(
        offenders.is_empty(),
        "neo-rpc should import neo_vm::OpCode directly instead of routing \
         through neo-core's compatibility module: {offenders:?}"
    );
}

#[test]
fn workspace_tests_and_benches_do_not_import_local_vm_runtime() {
    let workspace = workspace_root();
    let mut offenders = Vec::new();
    for dir in ["benches-package/benches", "tests/tests"] {
        collect_rs_files(&workspace.join(dir), &mut offenders);
    }

    // Workspace tests and benches may consume the neo-vm crate (directly or
    // through the neo_core::neo_vm facade); they must not touch the deleted
    // local VM implementation tree.
    const DEAD_PATHS: [&str; 6] = [
        "neo_core::neo_vm::jump_table",
        "neo_core::neo_vm::stack_item::",
        "neo_core::neo_vm::execution_engine",
        "neo_core::neo_vm::op_code",
        "crate::neo_vm::jump_table",
        "crate::neo_vm::stack_item::",
    ];

    offenders.retain(|path| {
        if path
            .file_name()
            .is_some_and(|name| name == "no_local_neo_vm_dependency.rs")
        {
            return false;
        }

        fs::read_to_string(path)
            .is_ok_and(|source| DEAD_PATHS.iter().any(|dead| source.contains(dead)))
    });

    assert!(
        offenders.is_empty(),
        "workspace-level tests and benchmarks must not import the deleted local VM \
         implementation tree: {offenders:?}"
    );
}

#[test]
fn manifest_modules_use_vm_runtime_stack_items() {
    let workspace = workspace_root();
    for relative in [
        "neo-smart-contract/src/manifest/contract_abi.rs",
        "neo-smart-contract/src/manifest/contract_event_descriptor.rs",
        "neo-smart-contract/src/manifest/contract_group.rs",
        "neo-smart-contract/src/manifest/contract_manifest.rs",
        "neo-smart-contract/src/manifest/contract_method_descriptor.rs",
        "neo-smart-contract/src/manifest/contract_parameter_definition.rs",
        "neo-smart-contract/src/manifest/contract_permission.rs",
        "neo-smart-contract/src/manifest/contract_permission_descriptor.rs",
        "neo-smart-contract/src/manifest/wild_card_container.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        assert!(
            imports_stack_item_via_neo_vm(&source),
            "{relative} should import StackItem through neo_vm"
        );
    }
}

#[test]
fn native_event_and_data_modules_use_vm_runtime_stack_items() {
    let workspace = workspace_root();
    for relative in [
        "neo-smart-contract/src/contract_state.rs",
        "neo-native-contracts/src/native/oracle_contract/events.rs",
        "neo-native-contracts/src/native/token_management/events.rs",
        "neo-native-contracts/src/native/token_management/stack_value.rs",
        "neo-native-contracts/src/native/token_management/state.rs",
        "neo-native-contracts/src/native/trimmed_block.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        assert!(
            imports_stack_item_via_neo_vm(&source),
            "{relative} should import StackItem through neo_vm"
        );
    }
}

#[test]
fn native_contract_modules_use_vm_runtime_stack_items() {
    let workspace = workspace_root();
    for relative in [
        "neo-native-contracts/src/native/contract_management/mod.rs",
        "neo-native-contracts/src/native/contract_management/tests.rs",
        "neo-native-contracts/src/native/fungible_token.rs",
        "neo-native-contracts/src/native/gas_token/mod.rs",
        "neo-native-contracts/src/native/interoperable_list.rs",
        "neo-native-contracts/src/native/ledger_contract/native_impl.rs",
        "neo-native-contracts/src/native/neo_token/mod.rs",
        "neo-native-contracts/src/native/notary/mod.rs",
        "neo-native-contracts/src/native/oracle_contract/response.rs",
        "neo-native-contracts/src/native/oracle_contract/storage.rs",
        "neo-native-contracts/src/native/policy_contract/mod.rs",
        "neo-native-contracts/src/native/policy_contract/tests.rs",
        "neo-native-contracts/src/native/role_management/mod.rs",
        "neo-native-contracts/src/native/std_lib/helpers.rs",
        "neo-native-contracts/src/native/std_lib/strings.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        assert!(
            imports_stack_item_via_neo_vm(&source),
            "{relative} should import StackItem through neo_vm"
        );
    }
}

#[test]
fn fuzz_script_parser_uses_direct_script_validation() {
    let workspace = workspace_root();
    let target = read_source(workspace.join("fuzz/fuzz_targets/fuzz_script_parse.rs"));

    assert!(
        target.contains("use neo_vm::validate_script;")
            && target.contains("validate_script(data, false)")
            && target.contains("validate_script(data, true)")
            && target.contains("script.has_instruction_at(0)"),
        "script parsing fuzz target should exercise the direct neo-vm-rs-backed bytecode \
         validator"
    );
    assert!(
        !target.contains("neo_core::neo_vm::Script")
            && !target.contains("neo_core::script_validation")
            && !target.contains("Script::new")
            && !target.contains("Script::new_relaxed"),
        "script parsing fuzz target should not construct the local neo_vm::Script"
    );
}

#[test]
fn fuzz_manifest_and_source_use_direct_vm_authority() {
    let workspace = workspace_root();
    let manifest = read_source(workspace.join("fuzz/Cargo.toml"));
    let target = read_source(workspace.join("fuzz/fuzz_targets/fuzz_script_parse.rs"));
    assert!(manifest.contains("neo-vm = { path = \"../neo-vm\", version = \"0.20.0\" }"));
    assert!(manifest.contains("path = \"fuzz_targets/fuzz_script_parse.rs\""));
    assert!(target.contains("use neo_vm::validate_script;") && !target.contains("neo_core::"));
    let probe = read_source(workspace.join("neo-probe/Cargo.toml"));
    assert!(
        probe.contains("neo-vm = { workspace = true }")
            && probe.contains("neo-smart-contract = { workspace = true }")
            && !probe.contains("neo-core")
    );
}

#[test]
fn primitive_interop_validation_uses_stackvalue_directly() {
    let workspace = workspace_root();
    for relative in [
        "neo-smart-contract/src/validator_attribute.rs",
        "neo-smart-contract/src/max_length_attribute.rs",
        "neo-smart-contract/src/interop_parameter_descriptor.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        assert!(
            source.contains("neo_vm::StackValue"),
            "{relative} should validate primitive interop payloads with \
             neo_vm::StackValue"
        );
        assert!(
            !source.contains("crate::neo_vm::StackItem"),
            "{relative} should not import local StackItem for primitive interop validation"
        );
    }

    let max_length = read_source(workspace.join("neo-smart-contract/src/max_length_attribute.rs"));
    assert!(
        max_length.contains("to_byte_string_bytes()"),
        "MaxLengthAttribute should use neo-vm-rs byte-string conversion rules for \
         primitive integer and boolean lengths"
    );

    let interop = fs::read_to_string(
        workspace.join("neo-smart-contract/src/interop_parameter_descriptor.rs"),
    )
    .unwrap();
    assert!(
        interop.contains("ConvertedValue::StackValue")
            && !interop.contains("ConvertedValue::StackItem"),
        "InteropParameterDescriptor should preserve generic values as \
         neo_vm::StackValue, not local StackItem"
    );
}

#[test]
fn local_vm_facade_does_not_keep_unused_exception_wrapper_modules() {
    let workspace = workspace_root();
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));

    for (module, symbol) in [
        ("bad_script_exception", "BadScriptException"),
        ("catchable_exception", "CatchableException"),
        ("vm_unhandled_exception", "VMUnhandledException"),
    ] {
        assert!(
            !workspace
                .join(format!("neo-core/src/neo_vm/{module}.rs"))
                .exists(),
            "unused local VM exception wrapper module {module}.rs should be deleted; \
             VmError is the runtime error boundary"
        );
        assert!(
            !vm_module.contains(&format!("pub mod {module}"))
                && !vm_module.contains(&format!("pub use {module}::{symbol}")),
            "neo_core::neo_vm should not expose unused exception wrapper {symbol}"
        );
    }
}

#[test]
fn local_vm_reference_counter_does_not_keep_unused_trait_shim() {
    let workspace = workspace_root();
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));
    let reference_counter = read_source(workspace.join("neo-vm/src/reference_counter.rs"));

    assert!(
        !workspace
            .join("neo-core/src/neo_vm/i_reference_counter.rs")
            .exists(),
        "IReferenceCounter should be deleted once ReferenceCounter directly owns the \
         reference-counting API"
    );
    assert!(
        !vm_module.contains("pub mod i_reference_counter")
            && !vm_module.contains("pub use i_reference_counter::IReferenceCounter"),
        "neo_core::neo_vm should not expose the unused IReferenceCounter shim"
    );
    assert!(
        !reference_counter.contains("use crate::neo_vm::i_reference_counter::IReferenceCounter")
            && !reference_counter.contains("impl IReferenceCounter for ReferenceCounter"),
        "ReferenceCounter should not implement an unused trait facade"
    );
}

#[test]
fn reference_graph_helpers_are_owned_by_neo_vm_rs() {
    let workspace = workspace_root();
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));
    let stack_item_module = read_source(workspace.join("neo-vm/src/stack_item/mod.rs"));
    let reference_counter = read_source(workspace.join("neo-vm/src/reference_counter.rs"));

    assert!(
        !workspace
            .join("neo-core/src/neo_vm/strongly_connected_components")
            .exists()
            && !vm_module.contains("pub mod strongly_connected_components"),
        "neo-core should not keep a local strongly_connected_components facade"
    );
    assert!(
        !workspace
            .join("neo-core/src/neo_vm/stack_item/stack_item_vertex.rs")
            .exists()
            && !stack_item_module.contains("pub mod stack_item_vertex")
            && !stack_item_module.contains("pub use stack_item_vertex"),
        "compound stack item id allocation should not keep a local stack_item_vertex facade"
    );
    assert!(
        reference_counter.contains("use crate::Tarjan;")
            || reference_counter.contains("use neo_vm::Tarjan;"),
        "ReferenceCounter should use the crate's shared Tarjan implementation directly"
    );

    let relative = "neo-vm/src/stack_item/map.rs";
    let source = read_source(workspace.join(relative));
    assert!(
        source.contains("next_stack_item_id")
            && !source.contains("stack_item_vertex::next_stack_item_id"),
        "{relative} should allocate compound ids through the crate helper directly"
    );

    let mut shared_tarjan = neo_vm::Tarjan::new();
    shared_tarjan.add_edge(1, 2);
    shared_tarjan.add_edge(2, 1);
    assert_eq!(shared_tarjan.find_components().len(), 1);
}

#[test]
fn local_stack_item_view_shims_are_removed() {
    let workspace = workspace_root();
    let stack_item_module = read_source(workspace.join("neo-vm/src/stack_item/mod.rs"));
    let compound_jump_table = read_source(workspace.join("neo-vm/src/jump_table/compound.rs"));

    for relative in [
        "neo-core/src/neo_vm/stack_item/primitive_type.rs",
        "neo-core/src/neo_vm/stack_item/compound_type.rs",
    ] {
        assert!(
            !workspace.join(relative).exists(),
            "{relative} should be removed; VM value behavior belongs in StackItem or neo-vm-rs"
        );
    }

    for symbol in [
        "pub mod primitive_type",
        "pub mod compound_type",
        "PrimitiveTypeExt",
        "CompoundTypeExt",
        "PrimitiveType",
        "CompoundType",
    ] {
        assert!(
            !stack_item_module.contains(symbol),
            "stack_item facade should not re-export local C# view shim {symbol}"
        );
    }

    assert!(
        !compound_jump_table.contains("as_primitive()")
            && !compound_jump_table.contains("primitive_type::PrimitiveTypeExt"),
        "compound opcode handlers should use StackItem/neo-vm-rs conversion helpers directly"
    );
}

#[test]
fn unused_primitive_stack_item_wrappers_are_removed() {
    let workspace = workspace_root();
    let stack_item_module = read_source(workspace.join("neo-vm/src/stack_item/mod.rs"));
    let stack_item = read_source(workspace.join("neo-vm/src/stack_item/stack_item.rs"));

    for relative in [
        "neo-core/src/neo_vm/stack_item/boolean.rs",
        "neo-core/src/neo_vm/stack_item/integer.rs",
        "neo-core/src/neo_vm/stack_item/null.rs",
    ] {
        assert!(
            !workspace.join(relative).exists(),
            "{relative} should be removed; primitive values should use StackItem or \
             neo_vm::StackValue directly"
        );
    }

    for symbol in [
        "pub mod boolean",
        "pub mod integer",
        "pub mod null",
        "pub use boolean::Boolean",
        "pub use integer::Integer",
        "pub use null::Null",
    ] {
        assert!(
            !stack_item_module.contains(symbol),
            "stack_item facade should not expose unused primitive wrapper {symbol}"
        );
    }

    assert!(
        !stack_item.contains("stack_item::integer::Integer::MAX_SIZE"),
        "StackItem should keep the Neo integer byte limit locally instead of depending on \
         an unused Integer wrapper"
    );
}

#[test]
fn unused_interop_interface_stack_item_wrapper_is_removed() {
    let workspace = workspace_root();
    let stack_item_module = read_source(workspace.join("neo-vm/src/stack_item/mod.rs"));
    let stack_item = read_source(workspace.join("neo-vm/src/stack_item/stack_item.rs"));

    assert!(
        !workspace
            .join("neo-core/src/neo_vm/stack_item/interop_interface.rs")
            .exists()
            && !stack_item_module.contains("pub mod interop_interface"),
        "neo-core should not keep the unused InteropInterfaceItem wrapper module"
    );
    assert!(
        stack_item.contains("pub trait InteropInterface")
            && stack_item_module.contains("pub use stack_item::InteropInterface"),
        "the live host-runtime InteropInterface trait should remain in StackItem"
    );
}

#[test]
fn nep11_token_ordering_does_not_use_local_bytestring_wrapper() {
    let workspace = workspace_root();
    let stack_item_module = read_source(workspace.join("neo-vm/src/stack_item/mod.rs"));
    let balance_key = read_source(
        workspace.join("neo-rpc/src/tokens_tracker/trackers/nep_11/nep11_balance_key.rs"),
    );
    let transfer_key = read_source(
        workspace.join("neo-rpc/src/tokens_tracker/trackers/nep_11/nep11_transfer_key.rs"),
    );
    let nep11_module =
        read_source(workspace.join("neo-rpc/src/tokens_tracker/trackers/nep_11/mod.rs"));

    assert!(
        !workspace
            .join("neo-core/src/neo_vm/stack_item/byte_string.rs")
            .exists()
            && !stack_item_module.contains("pub mod byte_string")
            && !stack_item_module.contains("pub use byte_string::ByteString"),
        "neo-core should not keep a local ByteString wrapper facade"
    );
    assert!(
        !balance_key.contains("ByteString::new")
            && !transfer_key.contains("ByteString::new")
            && balance_key.contains("token_id_integer")
            && transfer_key.contains("token_id_integer")
            && nep11_module.contains("BigInt::from_signed_bytes_le"),
        "NEP-11 token ordering should use direct signed little-endian token decoding"
    );
}

#[test]
fn local_vm_facade_does_not_keep_unused_debugger_module() {
    let workspace = workspace_root();
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));

    assert!(
        !workspace.join("neo-core/src/neo_vm/debugger.rs").exists(),
        "unused local Debugger wrapper should be deleted instead of widening the local \
         neo_vm facade"
    );
    assert!(
        !vm_module.contains("pub mod debugger")
            && !vm_module.contains("pub use debugger::Debugger"),
        "neo_core::neo_vm should not expose the unused Debugger facade"
    );
    assert!(
        !vm_module.contains("use neo_vm::{Debugger") && !vm_module.contains("Debugger::new"),
        "neo_core::neo_vm docs should not advertise a local Debugger facade after it is deleted"
    );
}

#[test]
fn local_vm_facade_does_not_keep_unused_application_engine_module() {
    let workspace = workspace_root();
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));

    assert!(
        !workspace
            .join("neo-core/src/neo_vm/application_engine.rs")
            .exists(),
        "unused local neo_vm ApplicationEngine facade should be deleted; production \
         blockchain-aware execution is owned by smart_contract::ApplicationEngine"
    );
    assert!(
        !vm_module.contains("pub mod application_engine;")
            && !vm_module.contains("pub use application_engine::{ApplicationEngine")
            && !vm_module.contains("NotificationEvent")
            && !vm_module.contains("TriggerType"),
        "neo_core::neo_vm should not expose local ApplicationEngine, NotificationEvent, \
         or TriggerType facades"
    );
    assert!(
        !vm_module.contains("## Using the `ApplicationEngine`")
            && !vm_module.contains("use neo_vm::{ApplicationEngine")
            && !vm_module.contains("ApplicationEngine"),
        "neo_core::neo_vm docs should not advertise the deleted local ApplicationEngine facade"
    );
}

#[test]
fn local_vm_facade_does_not_reexport_unused_internal_helpers() {
    let workspace = workspace_root();
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));

    for reexport in [
        "pub use jump_table::{InstructionHandler",
        "pub use jump_table::InstructionHandler",
        "VmInteropDescriptor as InteropDescriptor",
        "pub use strongly_connected_components::Tarjan",
    ] {
        assert!(
            !vm_module.contains(reexport),
            "neo_core::neo_vm should not publicly re-export unused internal helper via {reexport}"
        );
    }
}

#[test]
fn local_vm_test_tree_does_not_keep_unwired_or_empty_modules() {
    let workspace = workspace_root();
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));

    assert!(
        !workspace.join("neo-core/src/neo_vm/tests").exists(),
        "local VM test scaffolding should be deleted or moved into package-level tests; \
         ScriptBuilder tests belong with script_builder.rs, not neo_vm::tests"
    );
    assert!(
        !vm_module.contains("pub mod tests"),
        "neo_core::neo_vm should not expose a local tests module"
    );
}

#[test]
fn rpc_final_state_models_use_neo_vm_rs_vmstate_directly() {
    let workspace = workspace_root();
    for relative in [
        "neo-rpc/src/client/models/rpc_invoke_result.rs",
        "neo-rpc/src/client/models/rpc_application_log.rs",
        "neo-rpc/src/client/models/rpc_transaction.rs",
        "neo-rpc/src/client/models/vm_state_utils.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        assert!(
            source.contains("use neo_vm::VmState;"),
            "{relative} should use neo_vm::VmState directly for final RPC VM states"
        );
        assert!(
            !contains_neo_core_vmstate_import(&source),
            "{relative} should not import local neo_core::neo_vm::VMState for final RPC states"
        );
    }

    let helper = read_source(workspace.join("neo-rpc/src/client/models/vm_state_utils.rs"));
    assert!(
        helper.contains("pub fn vm_state_to_string(state: VmState)"),
        "RPC VM-state formatting should accept neo_vm::VmState directly"
    );
    assert!(
        helper.contains("pub fn vm_state_from_str(value: &str) -> Option<VmState>"),
        "RPC VM-state parsing should return neo_vm::VmState directly"
    );
    assert!(
        helper.contains("final_name()"),
        "RPC final-state parsing must map HALT/FAULT through neo-vm-rs"
    );
    assert!(
        !helper.contains("VMState::NONE")
            && !helper.contains("VMState::BREAK")
            && !helper.contains("\"NONE\"")
            && !helper.contains("\"BREAK\""),
        "RPC final-result state helper should not preserve local execution/debug states"
    );
}

#[test]
fn rpc_server_final_state_formatting_uses_neo_vm_rs_boundary() {
    let workspace = workspace_root();
    let helper = read_source(workspace.join("neo-rpc/src/server/smart_contract/helpers.rs"));

    assert!(
        helper.contains(".final_name()"),
        "server smart-contract helpers should use neo-vm-rs final-state helpers"
    );
    assert!(
        helper.contains("pub(super) fn final_rpc_vm_state_string"),
        "server smart-contract helpers should centralize final RPC VM-state formatting"
    );
    assert!(
        helper.contains(".final_name()"),
        "server final-state formatting should project local runtime VMState through \
         neo-vm-rs final-state semantics"
    );

    for relative in [
        "neo-rpc/src/server/smart_contract/invocation.rs",
        "neo-rpc/src/server/smart_contract/contract_verify.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        assert!(
            source.contains("final_rpc_vm_state_string"),
            "{relative} should format response state through the shared neo-vm-rs boundary"
        );
        assert!(
            !source.contains("format!(\"{vm_state:?}\")")
                && !source.contains("format!(\"{:?}\", state)"),
            "{relative} should not format local VMState directly for final RPC JSON"
        );
    }
}

#[test]
fn rpc_server_blockchain_tests_seed_native_state_through_stack_value() {
    let workspace = workspace_root();
    let source = read_source(workspace.join("neo-rpc/src/server/rpc_server_blockchain/tests.rs"));

    assert!(
        source.contains("use neo_vm::{OpCode, StackValue};")
            && source.contains("BinarySerializer::serialize_stack_value"),
        "RPC blockchain tests should seed native-contract storage through neo_vm::StackValue"
    );
    assert!(
        !source.contains("neo_core::neo_vm::StackItem")
            && !source.contains("BinarySerializer::serialize(&array")
            && !source.contains("BinarySerializer::serialize(&item")
            && !source.contains("BinarySerializer::serialize(\n        &invalid_item"),
        "RPC blockchain tests should not build persisted native-state payloads with local \
         StackItem"
    );
}

#[test]
fn rpc_client_stack_models_use_neo_vm_rs_stackvalue_directly() {
    let workspace = workspace_root();
    for relative in [
        "neo-rpc/src/client/utility.rs",
        "neo-rpc/src/client/utility/stack.rs",
        "neo-rpc/src/client/models/rpc_invoke_result.rs",
        "neo-rpc/src/client/models/rpc_application_log.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        assert!(
            source.contains("use neo_vm::StackValue;"),
            "{relative} should use neo_vm::StackValue directly for RPC stack payloads"
        );
        assert!(
            !source.contains("neo_core::neo_vm::StackItem")
                && !source.contains("neo_core::neo_vm::{StackItem")
                && !source.contains("neo_core::neo_vm::{OrderedDictionary")
                && !source.contains("neo_core::neo_vm::stack_item::InteropInterface"),
            "{relative} should not route RPC stack payloads through the local StackItem VM layer"
        );
    }

    let stack = read_source(workspace.join("neo-rpc/src/client/utility/stack.rs"));
    assert!(
        stack.contains("StackValue::BigInteger(integer.to_signed_bytes_le())"),
        "RPC stack integer parsing should preserve non-i64 integer strings with \
         neo-vm-rs BigInteger"
    );
    assert!(
        !stack.contains("OrderedDictionary") && !stack.contains("Script::new_relaxed"),
        "RPC stack JSON parsing should not construct local VM map/pointer compatibility objects"
    );
}

#[test]
fn rpc_server_imports_host_runtime_directly_from_neo_vm() {
    let workspace = workspace_root();
    for (relative, symbols) in [
        (
            "neo-rpc/src/server/diagnostic.rs",
            &["ExecutionContext", "Instruction"][..],
        ),
        (
            "neo-rpc/src/server/session.rs",
            &["StackItem", "InteropInterface"][..],
        ),
        (
            "neo-rpc/src/server/rpc_server_tokens_tracker/mod.rs",
            &["ScriptBuilder", "StackItem"][..],
        ),
        (
            "neo-rpc/src/server/smart_contract/helpers.rs",
            &[
                "NotifyEventArgs",
                "ScriptBuilder",
                "StackItem",
                "StackValue",
                "VmState",
                "stack_item_rpc_json_deferred_size_check",
            ][..],
        ),
        (
            "neo-rpc/src/server/smart_contract/invocation.rs",
            &["StackItem", "VmState"][..],
        ),
        (
            "neo-rpc/src/server/smart_contract/tests.rs",
            &[
                "InteropInterface",
                "BinarySerializer",
                "ExecutionEngineLimits",
                "Interoperable",
                "OpCode",
                "ScriptBuilder",
            ][..],
        ),
    ] {
        let source = read_source(workspace.join(relative));
        for forbidden in [
            "neo_core::neo_vm",
            "neo_core::vm_runtime",
            "neo_core::script_builder",
            "neo_core::smart_contract::binary_serializer",
        ] {
            assert!(
                !source.contains(forbidden),
                "{relative} must not use a core VM facade: {forbidden}"
            );
        }
        let direct_imports = source
            .split(';')
            .filter_map(|statement| {
                statement
                    .find("use neo_vm::")
                    .map(|start| &statement[start..])
            })
            .collect::<Vec<_>>()
            .join("\n");
        for symbol in symbols {
            assert!(
                direct_imports.contains(symbol),
                "{relative} must import {symbol} directly from neo_vm"
            );
        }
    }
}

#[test]
fn rpc_error_type_does_not_expose_local_vm_error() {
    let workspace = workspace_root();
    let rpc_error = read_source(workspace.join("neo-rpc/src/error.rs"));

    assert!(
        !rpc_error.contains("neo_core::neo_vm::VmError"),
        "neo-rpc should not expose local neo_vm::VmError through its public error type"
    );
    assert!(
        !rpc_error.contains("Vm(#[from]"),
        "neo-rpc should map local script-building errors explicitly instead of relying on \
         a blanket From<VmError> conversion"
    );
}

#[test]
fn rpc_server_invoke_arguments_push_neo_vm_rs_stackvalue_directly() {
    let workspace = workspace_root();
    let script_builder = read_source(workspace.join("neo-vm/src/script_builder.rs"));
    assert!(
        script_builder.contains("pub fn emit_push_stack_value")
            && script_builder.contains("StackValue"),
        "ScriptBuilder should accept StackValue directly for callers that no \
         longer need local StackItem identity"
    );

    let helpers = read_source(workspace.join("neo-rpc/src/server/smart_contract/helpers.rs"));
    assert!(
        helpers.contains("contract_parameter_to_stack_value")
            && helpers.contains("emit_push_stack_value(item)"),
        "RPC invoke argument script construction should convert contract parameters to \
         neo_vm::StackValue and push them directly"
    );
    assert!(
        !helpers.contains("contract_parameter_to_stack_item")
            && !helpers.contains("use neo_core::neo_vm::OrderedDictionary;"),
        "RPC invoke argument conversion should not allocate local StackItem maps before \
         script emission"
    );

    let contract_verify =
        read_source(workspace.join("neo-rpc/src/server/smart_contract/contract_verify.rs"));
    assert!(
        contract_verify.contains("contract_parameter_to_stack_value")
            && contract_verify.contains("emit_push_stack_value(&item)")
            && !contract_verify.contains("contract_parameter_to_stack_item")
            && !contract_verify.contains("emit_push_stack_item(item)"),
        "contract verification script construction should share the direct StackValue \
         push path"
    );
}

/// Returns true when `StackItem` enters scope through the shared VM boundary.
fn imports_stack_item_via_neo_vm(source: &str) -> bool {
    if source.contains("crate::neo_vm::StackItem") || source.contains("neo_vm::StackItem") {
        return true;
    }
    for prefix in ["use crate::neo_vm::{", "use neo_vm::{"] {
        let mut rest = source;
        while let Some(start) = rest.find(prefix) {
            let group = &rest[start + prefix.len()..];
            if let Some(end) = group.find('}') {
                if group[..end]
                    .split(',')
                    .any(|item| item.trim() == "StackItem")
                {
                    return true;
                }
                rest = &group[end..];
            } else {
                break;
            }
        }
    }
    source.contains("neo_vm::stack_item::*")
}

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tests crate lives below workspace root")
        .to_path_buf()
}

fn cargo_manifests(workspace: &Path) -> Vec<PathBuf> {
    let mut manifests = Vec::new();
    collect_manifests(workspace, &mut manifests);
    manifests
}

fn collect_manifests(dir: &Path, manifests: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();
        let name = entry.file_name();
        let name = name.to_string_lossy();

        if name == "target" || name == ".git" || name == "neo_csharp" {
            continue;
        }

        if path.is_dir() {
            // Nested repositories and linked worktrees are not this workspace.
            if path.join(".git").exists() || entry.file_type().unwrap().is_symlink() {
                continue;
            }
            collect_manifests(&path, manifests);
        } else if name == "Cargo.toml" {
            manifests.push(path);
        }
    }
}

fn collect_rs_files(dir: &Path, files: &mut Vec<PathBuf>) {
    for entry in fs::read_dir(dir).unwrap() {
        let entry = entry.unwrap();
        let path = entry.path();

        if path.is_dir() {
            collect_rs_files(&path, files);
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
}

fn read_source(path: impl AsRef<Path>) -> String {
    fs::read_to_string(path)
        .unwrap()
        .replace("\r\n", "\n")
        .replace('\r', "\n")
}

fn contains_neo_core_vmstate_import(source: &str) -> bool {
    source.contains("neo_core::neo_vm::VMState") || {
        let mut rest = source;
        while let Some(start) = rest.find("neo_core::neo_vm::{") {
            rest = &rest[start..];
            let import = rest.split_once(';').map_or(rest, |(import, _)| import);
            if import.contains("VMState") {
                return true;
            }
            rest = &rest["neo_core::neo_vm::{".len()..];
        }
        false
    }
}

fn execute_sign(value: BigInt) -> BigInt {
    let mut builder = ScriptBuilder::new();
    builder.emit_push_bigint(value).unwrap();
    builder.emit_opcode(OpCode::SIGN);
    builder.emit_opcode(OpCode::RET);

    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(Script::new_relaxed(builder.to_array()), -1, 0)
        .unwrap();
    let state = engine.execute();
    assert_eq!(state, VMState::HALT);

    engine.result_stack().peek(0).unwrap().as_int().unwrap()
}

fn execute_binary_int(opcode: OpCode, left: BigInt, right: BigInt) -> BigInt {
    let mut builder = ScriptBuilder::new();
    builder.emit_push_bigint(left).unwrap();
    builder.emit_push_bigint(right).unwrap();
    builder.emit_opcode(opcode);
    builder.emit_opcode(OpCode::RET);

    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(Script::new_relaxed(builder.to_array()), -1, 0)
        .unwrap();
    let state = engine.execute();
    assert_eq!(state, VMState::HALT);

    engine.result_stack().peek(0).unwrap().as_int().unwrap()
}

fn execute_binary_bool(opcode: OpCode, left: BigInt, right: BigInt) -> bool {
    let mut builder = ScriptBuilder::new();
    builder.emit_push_bigint(left).unwrap();
    builder.emit_push_bigint(right).unwrap();
    builder.emit_opcode(opcode);
    builder.emit_opcode(OpCode::RET);

    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(Script::new_relaxed(builder.to_array()), -1, 0)
        .unwrap();
    let state = engine.execute();
    assert_eq!(state, VMState::HALT);

    engine.result_stack().peek(0).unwrap().as_bool().unwrap()
}

fn execute_ternary_int(opcode: OpCode, first: BigInt, second: BigInt, third: BigInt) -> BigInt {
    let mut builder = ScriptBuilder::new();
    builder.emit_push_bigint(first).unwrap();
    builder.emit_push_bigint(second).unwrap();
    builder.emit_push_bigint(third).unwrap();
    builder.emit_opcode(opcode);
    builder.emit_opcode(OpCode::RET);

    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(Script::new_relaxed(builder.to_array()), -1, 0)
        .unwrap();
    let state = engine.execute();
    assert_eq!(state, VMState::HALT);

    engine.result_stack().peek(0).unwrap().as_int().unwrap()
}

fn execute_unary_int(opcode: OpCode, value: BigInt) -> BigInt {
    let mut builder = ScriptBuilder::new();
    builder.emit_push_bigint(value).unwrap();
    builder.emit_opcode(opcode);
    builder.emit_opcode(OpCode::RET);

    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(Script::new_relaxed(builder.to_array()), -1, 0)
        .unwrap();
    let state = engine.execute();
    assert_eq!(state, VMState::HALT);

    engine.result_stack().peek(0).unwrap().as_int().unwrap()
}

fn execute_within(value: BigInt, lower: BigInt, upper: BigInt) -> bool {
    let mut builder = ScriptBuilder::new();
    builder.emit_push_bigint(value).unwrap();
    builder.emit_push_bigint(lower).unwrap();
    builder.emit_push_bigint(upper).unwrap();
    builder.emit_opcode(OpCode::WITHIN);
    builder.emit_opcode(OpCode::RET);

    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(Script::new_relaxed(builder.to_array()), -1, 0)
        .unwrap();
    let state = engine.execute();
    assert_eq!(state, VMState::HALT);

    engine.result_stack().peek(0).unwrap().as_bool().unwrap()
}

fn execute_bool(builder: ScriptBuilder) -> bool {
    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(Script::new_relaxed(builder.to_array()), -1, 0)
        .unwrap();
    let state = engine.execute();
    assert_eq!(state, VMState::HALT);

    engine.result_stack().peek(0).unwrap().as_bool().unwrap()
}

fn execute_int(builder: ScriptBuilder) -> BigInt {
    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(Script::new_relaxed(builder.to_array()), -1, 0)
        .unwrap();
    let state = engine.execute();
    assert_eq!(state, VMState::HALT);

    engine.result_stack().peek(0).unwrap().as_int().unwrap()
}

fn execute_bytes(builder: ScriptBuilder) -> Vec<u8> {
    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(Script::new_relaxed(builder.to_array()), -1, 0)
        .unwrap();
    let state = engine.execute();
    assert_eq!(state, VMState::HALT);

    engine.result_stack().peek(0).unwrap().as_bytes().unwrap()
}
