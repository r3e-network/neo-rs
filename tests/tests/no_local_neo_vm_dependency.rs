use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use neo_core::neo_vm::{ExecutionEngine, Script, StackItem};
use neo_core::network::p2p::payloads::signer::Signer;
use neo_core::network::p2p::payloads::transaction::Transaction;
use neo_core::persistence::DataCache;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::script_builder::ScriptBuilder;
use neo_core::smart_contract::application_engine::{ApplicationEngine, TEST_MODE_GAS};
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::{UInt160, WitnessScope};
use neo_vm_rs::{
    ExceptionHandlingContext, ExceptionHandlingState, ExecutionEngineLimits, Instruction, OpCode,
    StackValue, VmOrderedDictionary, VmState as VMState,
};
use num_bigint::BigInt;

#[test]
fn neo_vm_host_crate_builds_on_external_neo_vm_rs() {
    // The stateful NeoVM host (execution engine, contexts, reference-counted
    // StackItem, jump tables) is the standalone `neo-vm` crate, extracted from
    // neo-core. The PURE VM semantics (OpCode/StackValue/ExecutionEngineLimits/
    // validators) still come from the external `neo-vm-rs`; the host builds ON it
    // and must never reimplement it. This test guards that layering.
    let workspace = workspace_root();
    let root_manifest = fs::read_to_string(workspace.join("Cargo.toml")).unwrap();

    assert!(
        root_manifest.contains("neo-vm-rs"),
        "root workspace dependencies must keep the external neo-vm-rs pure VM"
    );
    assert!(
        root_manifest.contains("\"neo-vm\""),
        "root workspace should include the extracted neo-vm host crate as a member"
    );

    let host_manifest =
        fs::read_to_string(workspace.join("neo-vm/Cargo.toml")).expect("neo-vm/Cargo.toml exists");
    assert!(
        host_manifest.contains("name = \"neo-vm\""),
        "the neo-vm host crate manifest should declare the neo-vm package"
    );
    assert!(
        host_manifest.contains("neo-vm-rs"),
        "the neo-vm host crate must depend on the external neo-vm-rs pure VM, not reimplement it"
    );
}

#[test]
fn production_script_helpers_use_shared_opcode_enum() {
    let workspace = workspace_root();
    for relative_path in [
        "neo-core/src/witness.rs",
        "neo-core/src/smart_contract/helper.rs",
        "neo-core/src/smart_contract/contract_state.rs",
        "neo-core/src/wallets/key_pair.rs",
    ] {
        let source = read_source(workspace.join(relative_path));

        assert!(
            source.contains("OpCode::"),
            "{relative_path} should construct or validate VM scripts through neo-vm-rs::OpCode"
        );
        for duplicate in [
            "0x0C ||  // OpCode.PUSHDATA1",
            "script[0] == 0x0C",
            "script[35] == 0x41",
            "script[len - 5] == 0x41",
            "script.push(0x0C)",
            "script.push(0x0c)",
            "script.push(0x41)",
            "vec![0x40]",
        ] {
            assert!(
                !source.contains(duplicate),
                "{relative_path} must not duplicate NeoVM opcode bytes: {duplicate}"
            );
        }
    }
}

#[test]
fn application_engine_executes_syscall_free_scripts_through_neo_vm_rs() {
    let workspace = workspace_root();
    let load_execute = read_source(
        workspace.join("neo-core/src/smart_contract/application_engine/load_execute_storage.rs"),
    );
    let external_vm = read_source(
        workspace.join("neo-core/src/smart_contract/application_engine/external_vm.rs"),
    );

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
    let external_vm = read_source(
        workspace.join("neo-core/src/smart_contract/application_engine/external_vm.rs"),
    );

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
    let external_vm = read_source(
        workspace.join("neo-core/src/smart_contract/application_engine/external_vm.rs"),
    );
    assert!(
        external_vm.contains("System.Runtime.GetScriptContainer")
            && external_vm.contains("to_stack_value()"),
        "script container projection should be handled directly through neo-vm-rs StackValue"
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
    let external_vm = read_source(
        workspace.join("neo-core/src/smart_contract/application_engine/external_vm.rs"),
    );
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
    let limits_path = workspace.join("neo-vm/src/execution_engine_limits.rs");
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
        engine_module.contains("use neo_vm_rs::{ExecutionEngineLimits, VmState as VMState};"),
        "local execution engine internals should import ExecutionEngineLimits directly \
         from neo-vm-rs"
    );
    assert_eq!(
        limits.max_stack_size,
        neo_vm_rs::DEFAULT_MAX_STACK_DEPTH as u32,
        "ExecutionEngineLimits should use neo-vm-rs for the default stack depth"
    );
    assert_eq!(
        limits.max_invocation_stack_size,
        neo_vm_rs::DEFAULT_MAX_INVOCATION_DEPTH as u32,
        "ExecutionEngineLimits should use neo-vm-rs for the default invocation depth"
    );
    assert_eq!(
        limits.max_item_size,
        (u16::MAX as u32) * 2,
        "ExecutionEngineLimits.MaxItemSize must equal C#'s ushort.MaxValue * 2 = 131070"
    );
    assert_eq!(
        limits.max_comparable_size, 65536,
        "ExecutionEngineLimits.MaxComparableSize must equal C#'s 65536"
    );
    assert_eq!(limits.max_shift, 256);
    assert_eq!(limits.max_try_nesting_depth, 16);
    assert!(limits.catch_engine_exceptions);
}

#[test]
fn exception_handling_facades_are_removed() {
    let workspace = workspace_root();
    let context_path = workspace.join("neo-vm/src/exception_handling_context.rs");
    let state_path = workspace.join("neo-vm/src/exception_handling_state.rs");
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));
    let execution_context =
        read_source(workspace.join("neo-vm/src/execution_context/context.rs"));
    let exception_runtime =
        read_source(workspace.join("neo-vm/src/execution_engine/exception.rs"));
    let mut context = ExceptionHandlingContext::new(-1, 42);

    assert!(
        !context_path.exists()
            && !state_path.exists()
            && !vm_module.contains("pub mod exception_handling_context")
            && !vm_module.contains("pub mod exception_handling_state")
            && !vm_module.contains("ExceptionHandlingContext")
            && !vm_module.contains("ExceptionHandlingState"),
        "neo-core should not keep local exception handling facades"
    );
    assert!(
        execution_context.contains("use neo_vm_rs::ExceptionHandlingContext;")
            && execution_context.contains("use neo_vm_rs::ExceptionHandlingState;")
            && exception_runtime.contains("use neo_vm_rs::ExceptionHandlingContext;")
            && exception_runtime.contains("use neo_vm_rs::ExceptionHandlingState;"),
        "local runtime should import exception frame types directly from neo-vm-rs"
    );

    assert_eq!(context.catch_pointer(), -1);
    assert_eq!(context.finally_pointer(), 42);
    assert_eq!(context.state(), ExceptionHandlingState::Try);
    context.set_state(ExceptionHandlingState::Finally);
    assert!(context.is_in_exception());
}

#[test]
fn neo_core_ordered_dictionary_facade_is_removed() {
    let workspace = workspace_root();
    let dictionary_path = workspace.join("neo-vm/src/collections/ordered_dictionary.rs");
    let collections_module_path = workspace.join("neo-vm/src/collections/mod.rs");
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));
    let map_source = read_source(workspace.join("neo-vm/src/stack_item/map.rs"));
    let stack_item_source =
        read_source(workspace.join("neo-vm/src/stack_item/stack_item.rs"));
    let helper_source =
        read_source(workspace.join("neo-core/src/smart_contract/application_engine_helper.rs"));
    let serializer_source =
        read_source(workspace.join("neo-core/src/smart_contract/binary_serializer.rs"));

    let mut dictionary = VmOrderedDictionary::new();
    dictionary.insert(3, 30);
    dictionary.insert(1, 10);
    dictionary.insert(2, 20);

    assert!(
        !dictionary_path.exists()
            && !collections_module_path.exists()
            && !vm_module.contains("pub mod collections")
            && !vm_module.contains("OrderedDictionary"),
        "neo-core should not keep a local VmOrderedDictionary facade under neo_vm"
    );
    assert_eq!(
        dictionary
            .iter()
            .map(|(key, value)| (*key, *value))
            .collect::<Vec<_>>(),
        vec![(3, 30), (1, 10), (2, 20)],
        "shared VmOrderedDictionary should preserve insertion order"
    );
    assert!(
        map_source.contains("use neo_vm_rs::VmOrderedDictionary;")
            && stack_item_source.contains("use neo_vm_rs::{StackValue, VmOrderedDictionary};"),
        "local stack item map code should use neo-vm-rs VmOrderedDictionary directly"
    );
    assert!(
        helper_source.contains("use neo_vm_rs::VmOrderedDictionary;")
            && serializer_source.contains("neo_vm_rs::VmOrderedDictionary::new()"),
        "smart-contract helpers should not allocate maps through the local ordered dictionary copy"
    );
}

#[test]
fn neo_core_does_not_reexport_opcode_through_vm_facade() {
    let workspace = workspace_root();
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));

    assert!(
        !vm_module.contains("pub use neo_vm_rs::OpCode;"),
        "neo_core::neo_vm should not keep an OpCode facade; callers should import \
         neo_vm_rs::OpCode directly"
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
        !workspace.join("neo-vm/src/op_code").exists(),
        "neo-vm/src/op_code should be deleted once OpCode comes from neo-vm-rs"
    );
}

#[test]
fn neo_core_vm_internals_import_opcode_from_neo_vm_rs() {
    let workspace = workspace_root();
    let vm_dir = workspace.join("neo-vm/src");
    let mut offenders = Vec::new();
    collect_rs_files(&vm_dir, &mut offenders);
    offenders.retain(|path| {
        fs::read_to_string(path)
            .map(|source| source.contains("use crate::neo_vm::op_code::OpCode;"))
            .unwrap_or(false)
    });

    assert!(
        offenders.is_empty(),
        "neo-core VM internals should import neo_vm_rs::OpCode directly, not \
         through the compatibility op_code module: {offenders:?}"
    );
}

#[test]
fn neo_core_sources_do_not_use_legacy_opcode_module_path() {
    let workspace = workspace_root();
    let core_dir = workspace.join("neo-core/src");
    let mut offenders = Vec::new();
    collect_rs_files(&core_dir, &mut offenders);
    offenders.retain(|path| {
        fs::read_to_string(path)
            .map(|source| {
                source.contains("op_code::OpCode")
                    || source.contains("crate::neo_vm::OpCode")
                    || contains_crate_neo_vm_opcode_import(&source)
            })
            .unwrap_or(false)
    });

    assert!(
        offenders.is_empty(),
        "neo-core sources should use neo_vm_rs::OpCode directly, not a \
         neo_vm OpCode facade or legacy op_code module path: {offenders:?}"
    );
}

#[test]
fn neo_core_vm_facade_only_reexports_opcode_from_neo_vm_rs() {
    let workspace = workspace_root();
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));

    assert!(
        !vm_module.contains("pub use neo_vm_rs"),
        "neo_core::neo_vm should not facade re-export neo_vm_rs symbols; \
         callers should import them directly from neo_vm_rs"
    );

    for symbol in [
        "ExecutionResult",
        "StackValue",
        "SyscallProvider",
        "VmContext",
        "interpret",
        "interop_hash",
        "syscall_arg_count",
    ] {
        assert!(
            !vm_module.contains(symbol),
            "neo_core::neo_vm should not facade re-export neo_vm_rs::{symbol}; \
             callers should import it directly from neo_vm_rs"
        );
    }
}

#[test]
fn native_contract_static_syscall_hash_uses_neo_vm_rs_directly() {
    let workspace = workspace_root();
    let native_contract =
        read_source(workspace.join("neo-core/src/smart_contract/native/native_contract.rs"));

    assert!(
        native_contract.contains("neo_vm_rs::interop_hash(\"System.Contract.CallNative\")"),
        "static native contract syscall hashing should call neo_vm_rs::interop_hash directly"
    );
    assert!(
        !native_contract.contains("ScriptBuilder::hash_syscall(\"System.Contract.CallNative\")"),
        "static native contract syscall hashing should not route through ScriptBuilder validation"
    );
}

#[test]
fn interop_service_hashes_syscalls_with_neo_vm_rs_directly() {
    let workspace = workspace_root();
    let interop_service =
        read_source(workspace.join("neo-vm/src/interop_service.rs"));

    assert!(
        interop_service.contains("neo_vm_rs::interop_hash"),
        "InteropService should compute syscall hashes with neo-vm-rs directly"
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
    let helper =
        read_source(workspace.join("neo-core/src/smart_contract/helper.rs"));

    assert!(
        helper.contains("neo_vm_rs::interop_hash(name).to_le_bytes()"),
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
    let descriptor =
        read_source(workspace.join("neo-core/src/smart_contract/interop_descriptor.rs"));

    assert!(
        descriptor.contains("neo_vm_rs::interop_hash(&self.name)"),
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
    let script_builder =
        read_source(workspace.join("neo-core/src/script_builder.rs"));

    assert!(
        script_builder.matches("neo_vm_rs::encode_integer").count() >= 2,
        "ScriptBuilder should reuse neo-vm-rs integer encoding for direct i64 pushes \
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
    let script_builder =
        read_source(workspace.join("neo-core/src/script_builder.rs"));

    assert!(
        script_builder.contains("op.byte()"),
        "ScriptBuilder's central opcode emitter should use neo-vm-rs OpCode::byte()"
    );
    assert!(
        !script_builder.contains("self.script.push(op as u8)"),
        "ScriptBuilder should not hand-cast OpCode in its central emitter"
    );
}

#[test]
fn script_builder_derives_opcode_bytes_from_neo_vm_rs_metadata() {
    let workspace = workspace_root();
    let script_builder =
        read_source(workspace.join("neo-core/src/script_builder.rs"));

    assert!(
        script_builder.contains("OpCode::PUSH0.byte()"),
        "ScriptBuilder should derive small-integer PUSH opcodes from neo-vm-rs \
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
            "ScriptBuilder should use neo-vm-rs OpCode byte metadata instead of {cast}"
        );
    }
}

#[test]
fn smart_contract_script_helpers_use_neo_vm_rs_opcode_byte_metadata() {
    let workspace = workspace_root();
    for relative in [
        "neo-core/src/smart_contract/helper.rs",
        "neo-core/src/wallets/helper.rs",
        "neo-core/src/smart_contract/application_engine/interop_host.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative))
            .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"));
        assert!(
            source.contains(".byte()"),
            "{relative} should use neo-vm-rs OpCode::byte() metadata for script bytes"
        );
        for cast in [
            "OpCode::PUSHDATA1 as u8",
            "OpCode::SYSCALL as u8",
            "OpCode::PUSH0 as u8",
            "OpCode::PUSH1 as u8",
            "OpCode::PUSH16 as u8",
            "instruction.opcode as u8",
        ] {
            assert!(
                !source.contains(cast),
                "{relative} should use neo-vm-rs OpCode byte metadata instead of {cast}"
            );
        }
    }
}

#[test]
fn witness_and_rpc_script_bytes_use_neo_vm_rs_opcode_metadata() {
    let workspace = workspace_root();
    for relative in [
        "neo-core/src/network/p2p/payloads/transaction/verification.rs",
        "neo-consensus/src/service/helpers/signatures.rs",
        "neo-node/src/consensus.rs",
        "neo-core/src/state_service/state_root.rs",
        "neo-core/src/state_service/state_store.rs",
        "neo-core/src/state_service/verification.rs",
        "neo-node/src/hsm_wallet.rs",
        "neo-node/src/tee_wallet.rs",
        "neo-rpc/src/server/rpc_server_node/mod.rs",
        "neo-rpc/src/server/rpc_server_wallet/mod.rs",
        "neo-rpc/src/server/smart_contract/contract_verify.rs",
        "neo-rpc/src/client/wallet_api.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative))
            .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"));
        for cast in [
            "OpCode::PUSHDATA1 as u8",
            "OpCode::PUSH0 as u8",
            "OpCode::PUSH1 as u8",
            "OpCode::RET as u8",
            "OpCode::ASSERT as u8",
            "neo_vm_rs::OpCode::PUSHDATA1 as u8",
        ] {
            assert!(
                !source.contains(cast),
                "{relative} should use neo-vm-rs OpCode::byte() metadata instead of {cast}"
            );
        }
    }
}

#[test]
fn remaining_production_script_bytes_use_neo_vm_rs_opcode_metadata() {
    let workspace = workspace_root();
    for relative in [
        "benches-package/benches/vm_execution.rs",
        "neo-core/src/builders/mod.rs",
        "neo-core/src/ledger/genesis.rs",
        "neo-core/src/wallets/wallet_account.rs",
        "neo-rpc/src/server/session.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative))
            .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"));
        for cast in [
            "OpCode::PUSHDATA1 as u8",
            "OpCode::PUSH1 as u8",
            "OpCode::ADD as u8",
            "OpCode::DROP as u8",
            "OpCode::NOP as u8",
            "OpCode::RET as u8",
        ] {
            assert!(
                !source.contains(cast),
                "{relative} should use neo-vm-rs OpCode::byte() metadata instead of {cast}"
            );
        }
    }
}

#[test]
fn workspace_script_bytes_use_neo_vm_rs_opcode_metadata() {
    let workspace = workspace_root();
    let mut files = Vec::new();
    for dir in [
        "benches-package/benches",
        "fuzz/fuzz_targets",
        "neo-consensus/src",
        "neo-core/src",
        "neo-core/tests",
        "neo-node/src",
        "neo-node/tests",
        "neo-rpc/src",
        "neo-rpc/tests",
        "tests/tests",
    ] {
        collect_rs_files(&workspace.join(dir), &mut files);
    }

    let mut offenders = Vec::new();
    for path in files {
        if path
            .file_name()
            .is_some_and(|name| name == "no_local_neo_vm_dependency.rs")
        {
            continue;
        }

        let source = fs::read_to_string(&path).unwrap();
        for (line_index, line) in source.lines().enumerate() {
            if line_contains_opcode_repr_cast(line) {
                let relative = path.strip_prefix(&workspace).unwrap_or(&path);
                offenders.push(format!("{}:{}", relative.display(), line_index + 1));
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "script byte construction should use neo-vm-rs OpCode::byte() metadata instead \
         of enum representation casts: {offenders:?}"
    );
}

#[test]
fn vm_state_byte_serialization_uses_neo_vm_rs_mapping() {
    let workspace = workspace_root();
    let vm_state_path = workspace.join("neo-vm/src/vm_state.rs");
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));
    let engine_module = read_source(workspace.join("neo-vm/src/execution_engine/mod.rs"));

    assert!(
        !vm_state_path.exists()
            && !vm_module.contains("pub mod vm_state")
            && !vm_module.contains("pub use vm_state::VMState"),
        "neo-core should not keep a local VMState facade"
    );
    assert!(
        engine_module.contains("use neo_vm_rs::{ExecutionEngineLimits, VmState as VMState};"),
        "local execution engine internals should import VmState directly from neo-vm-rs"
    );

    for relative in [
        "neo-core/src/smart_contract/native/ledger_contract/state.rs",
        "neo-core/src/smart_contract/native/transaction_state.rs",
        "neo-rpc/src/server/rpc_server_node/mod.rs",
        "neo-oracle-service/src/service/tests/response_tx.rs",
        "neo-core/tests/ledger_contract_tests.rs",
        "neo-core/tests/p2p_payloads_csharp_tests.rs",
        "neo-core/tests/runtime_syscall_tests.rs",
        "neo-rpc/src/server/routes/mod.rs",
        "neo-rpc/src/server/rpc_server_blockchain/tests.rs",
        "neo-rpc/src/server/rpc_server_wallet/tests.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative))
            .unwrap_or_else(|error| panic!("failed to read {relative}: {error}"));
        for cast in [
            "VMState::NONE as u8",
            "VMState::HALT as u8",
            "VMState::FAULT as u8",
            "VMState::BREAK as u8",
            "vm_state as u8",
            "self.state as u8",
        ] {
            assert!(
                !source.contains(cast),
                "{relative} should use VMState::to_byte/from_byte instead of {cast}"
            );
        }
    }
}

#[test]
fn non_vm_layers_import_shared_vm_scalars_directly() {
    let workspace = workspace_root();
    let mut offenders = Vec::new();

    for root in [
        workspace.join("neo-core/src"),
        workspace.join("neo-core/tests"),
        workspace.join("neo-rpc/src"),
        workspace.join("neo-rpc/tests"),
    ] {
        let mut files = Vec::new();
        collect_rs_files(&root, &mut files);
        for path in files {
            let relative = path.strip_prefix(&workspace).unwrap().to_string_lossy();
            let normalized_relative = relative.replace('\\', "/");
            if normalized_relative.starts_with("neo-vm/src") {
                continue;
            }

            let source = read_source(&path);
            let imports_local_vm_state = source.contains("crate::neo_vm::vm_state::VMState")
                || source.contains("crate::neo_vm::VMState")
                || source.contains("neo_core::neo_vm::vm_state::VMState")
                || source.contains("neo_core::neo_vm::VMState")
                || contains_braced_neo_vm_import(&source, "crate::neo_vm::{", "VMState")
                || contains_braced_neo_vm_import(&source, "neo_core::neo_vm::{", "VMState");
            let imports_local_limits = source
                .contains("crate::neo_vm::execution_engine_limits::ExecutionEngineLimits")
                || source.contains("crate::neo_vm::ExecutionEngineLimits")
                || source
                    .contains("neo_core::neo_vm::execution_engine_limits::ExecutionEngineLimits")
                || source.contains("neo_core::neo_vm::ExecutionEngineLimits")
                || contains_braced_neo_vm_import(
                    &source,
                    "crate::neo_vm::{",
                    "ExecutionEngineLimits",
                )
                || contains_braced_neo_vm_import(
                    &source,
                    "neo_core::neo_vm::{",
                    "ExecutionEngineLimits",
                );
            let imports_local_exception_frames = source
                .contains("crate::neo_vm::exception_handling_context::ExceptionHandlingContext")
                || source
                    .contains("crate::neo_vm::exception_handling_state::ExceptionHandlingState")
                || source.contains(
                    "neo_core::neo_vm::exception_handling_context::ExceptionHandlingContext",
                )
                || source
                    .contains("neo_core::neo_vm::exception_handling_state::ExceptionHandlingState")
                || source.contains("crate::neo_vm::ExceptionHandlingContext")
                || source.contains("crate::neo_vm::ExceptionHandlingState")
                || source.contains("neo_core::neo_vm::ExceptionHandlingContext")
                || source.contains("neo_core::neo_vm::ExceptionHandlingState")
                || contains_braced_neo_vm_import(
                    &source,
                    "crate::neo_vm::{",
                    "ExceptionHandlingContext",
                )
                || contains_braced_neo_vm_import(
                    &source,
                    "crate::neo_vm::{",
                    "ExceptionHandlingState",
                )
                || contains_braced_neo_vm_import(
                    &source,
                    "neo_core::neo_vm::{",
                    "ExceptionHandlingContext",
                )
                || contains_braced_neo_vm_import(
                    &source,
                    "neo_core::neo_vm::{",
                    "ExceptionHandlingState",
                );
            let imports_local_stack_item_type = source
                .contains("crate::neo_vm::stack_item::stack_item_type::StackItemType")
                || source.contains("crate::neo_vm::stack_item::StackItemType")
                || source.contains("crate::neo_vm::StackItemType")
                || source.contains("neo_core::neo_vm::stack_item::stack_item_type::StackItemType")
                || source.contains("neo_core::neo_vm::stack_item::StackItemType")
                || source.contains("neo_core::neo_vm::StackItemType")
                || contains_braced_neo_vm_import(&source, "crate::neo_vm::{", "StackItemType")
                || contains_braced_neo_vm_import(&source, "neo_core::neo_vm::{", "StackItemType")
                || contains_braced_neo_vm_import(
                    &source,
                    "crate::neo_vm::stack_item::{",
                    "StackItemType",
                )
                || contains_braced_neo_vm_import(
                    &source,
                    "neo_core::neo_vm::stack_item::{",
                    "StackItemType",
                );

            if imports_local_vm_state
                || imports_local_limits
                || imports_local_exception_frames
                || imports_local_stack_item_type
            {
                offenders.push(normalized_relative);
            }
        }
    }

    assert!(
        offenders.is_empty(),
        "non-VM layers should import shared VM scalar types from neo_vm_rs directly: \
         {offenders:?}"
    );
}

#[test]
fn vm_dispatch_uses_neo_vm_rs_opcode_byte_metadata() {
    let workspace = workspace_root();
    let jump_table =
        read_source(workspace.join("neo-vm/src/jump_table/mod.rs"));
    let execution =
        read_source(workspace.join("neo-vm/src/execution_engine/execution.rs"));

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
    let script_builder =
        read_source(workspace.join("neo-core/src/script_builder.rs"));

    assert!(
        script_builder.contains("neo_vm_rs::interop_hash(api)"),
        "ScriptBuilder::emit_syscall should hash with neo-vm-rs directly"
    );
    assert!(
        !script_builder.contains("pub fn hash_syscall")
            && !script_builder.contains("Self::hash_syscall")
            && !script_builder.contains("ScriptBuilder::hash_syscall"),
        "ScriptBuilder should not expose a redundant syscall-hash wrapper; callers can \
         use neo_vm_rs::interop_hash directly"
    );
}

#[test]
fn script_builder_does_not_accept_local_stackitem_bridge() {
    let workspace = workspace_root();
    let script_builder =
        read_source(workspace.join("neo-core/src/script_builder.rs"));

    assert!(
        !script_builder.contains("use crate::neo_vm::stack_item::StackItem")
            && !script_builder.contains("emit_push_stack_item")
            && !script_builder.contains("stack_item_to_stack_value"),
        "ScriptBuilder should accept neo_vm_rs::StackValue directly instead of keeping \
         a local StackItem conversion bridge"
    );

    let mut offenders = Vec::new();
    for dir in [
        "neo-core/src",
        "neo-rpc/src",
        "neo-node/src",
        "neo-consensus/src",
        "tests/tests",
        "neo-core/tests",
    ] {
        collect_rs_files(&workspace.join(dir), &mut offenders);
    }
    offenders.retain(|path| {
        if path
            .file_name()
            .is_some_and(|name| name == "no_local_neo_vm_dependency.rs")
        {
            return false;
        }

        fs::read_to_string(path)
            .map(|source| source.contains("emit_push_stack_item"))
            .unwrap_or(false)
    });

    assert!(
        offenders.is_empty(),
        "script-building callers should push neo_vm_rs::StackValue directly instead of \
         routing through ScriptBuilder::emit_push_stack_item: {offenders:?}"
    );
}

#[test]
fn witness_rules_project_through_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let mut witness_sources = vec![read_source(workspace.join("neo-core/src/witness_rule.rs"))];
    let mut witness_module_files = Vec::new();
    collect_rs_files(
        &workspace.join("neo-core/src/witness_rule"),
        &mut witness_module_files,
    );
    witness_module_files.sort();
    for path in witness_module_files {
        if path.file_name().and_then(|name| name.to_str()) == Some("tests.rs") {
            continue;
        }
        witness_sources.push(read_source(path));
    }
    let witness_rule = witness_sources.join("\n");

    assert!(
        witness_rule.contains("use neo_vm_rs::StackValue;"),
        "witness rules should import neo_vm_rs::StackValue directly for their data \
         projection boundary"
    );
    assert!(
        witness_rule
            .matches("pub fn to_stack_value(&self) -> StackValue")
            .count()
            >= 2,
        "WitnessCondition and WitnessRule should both expose direct StackValue projections"
    );
    assert!(
        witness_rule
            .matches("StackItem::try_from(self.to_stack_value())")
            .count()
            >= 2,
        "WitnessCondition::to_stack_item and WitnessRule::to_stack_item should adapt \
         from the direct neo-vm-rs StackValue projection"
    );
    for local_builder in [
        "StackItem::from_array",
        "StackItem::from_int",
        "StackItem::from_bool",
        "StackItem::from_byte_string",
    ] {
        assert!(
            !witness_rule.contains(local_builder),
            "witness rules should not hand-build stack-item shapes with {local_builder}; \
             build neo_vm_rs::StackValue first"
        );
    }
}

#[test]
fn signer_stack_projection_uses_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let signer =
        read_source(workspace.join("neo-core/src/network/p2p/payloads/signer.rs"));

    assert!(
        signer.contains("use neo_vm_rs::StackValue;"),
        "Signer should import neo_vm_rs::StackValue directly for its data projection boundary"
    );
    assert!(
        signer.contains("pub fn to_stack_value(&self) -> StackValue"),
        "Signer should expose a direct StackValue projection for RPC/native data callers"
    );
    assert!(
        signer.contains("StackItem::try_from(self.to_stack_value())"),
        "Signer::to_stack_item should adapt from the direct neo-vm-rs StackValue projection"
    );
    assert!(
        !signer.contains("WitnessRule::to_stack_item"),
        "Signer should compose witness rules through WitnessRule::to_stack_value instead of \
         bouncing through local StackItem values"
    );
    for local_builder in [
        "StackItem::from_array",
        "StackItem::from_int",
        "StackItem::from_byte_string",
    ] {
        assert!(
            !signer.contains(local_builder),
            "Signer should not hand-build stack-item shapes with {local_builder}; build \
             neo_vm_rs::StackValue first"
        );
    }
}

#[test]
fn transaction_stack_projection_uses_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let transaction = fs::read_to_string(
        workspace.join("neo-core/src/network/p2p/payloads/transaction/traits.rs"),
    )
    .unwrap();

    assert!(
        transaction.contains("use neo_vm_rs::StackValue;"),
        "Transaction should import neo_vm_rs::StackValue directly for its data projection boundary"
    );
    assert!(
        transaction.contains("pub fn to_stack_value(&self) -> Result<StackValue, CoreError>"),
        "Transaction should expose a direct StackValue projection for native/RPC data callers"
    );
    assert!(
        transaction.contains("StackItem::try_from(self.to_stack_value()?)"),
        "Transaction::to_stack_item should adapt from the direct neo-vm-rs StackValue projection"
    );
    for local_builder in [
        "StackItem::from_array",
        "StackItem::from_int",
        "StackItem::from_byte_string",
    ] {
        assert!(
            !transaction.contains(local_builder),
            "Transaction should not hand-build stack-item shapes with {local_builder}; build \
             neo_vm_rs::StackValue first"
        );
    }
}

#[test]
fn contract_parameter_definition_projection_uses_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let parameter_definition = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/manifest/contract_parameter_definition.rs"),
    )
    .unwrap();

    assert!(
        parameter_definition.contains("use neo_vm_rs::StackValue;"),
        "ContractParameterDefinition should import neo_vm_rs::StackValue directly for \
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
             {local_builder}; build neo_vm_rs::StackValue first"
        );
    }
}

#[test]
fn wild_card_container_projection_uses_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let wild_card_container = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/manifest/wild_card_container.rs"),
    )
    .unwrap();

    assert!(
        wild_card_container.contains("use neo_vm_rs::StackValue;"),
        "WildCardContainer should import neo_vm_rs::StackValue directly for \
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
             {local_builder}; build neo_vm_rs::StackValue first"
        );
    }
}

#[test]
fn manifest_event_and_method_descriptors_project_through_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let event_descriptor = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/manifest/contract_event_descriptor.rs"),
    )
    .unwrap();
    let method_descriptor = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/manifest/contract_method_descriptor.rs"),
    )
    .unwrap();

    for (name, source) in [
        ("ContractEventDescriptor", event_descriptor.as_str()),
        ("ContractMethodDescriptor", method_descriptor.as_str()),
    ] {
        assert!(
            source.contains("use neo_vm_rs::StackValue;"),
            "{name} should import neo_vm_rs::StackValue directly for manifest projection"
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
                 neo_vm_rs::StackValue first"
            );
        }
    }
}

#[test]
fn manifest_permissions_project_through_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let permission_descriptor = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/manifest/contract_permission_descriptor.rs"),
    )
    .unwrap();
    let permission = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/manifest/contract_permission.rs"),
    )
    .unwrap();

    assert!(
        permission_descriptor.contains("use neo_vm_rs::StackValue;"),
        "ContractPermissionDescriptor should import neo_vm_rs::StackValue directly for \
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
             {local_builder}; build neo_vm_rs::StackValue first"
        );
    }

    assert!(
        permission.contains("use neo_vm_rs::StackValue;"),
        "ContractPermission should import neo_vm_rs::StackValue directly for manifest projection"
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
        "ContractPermission should not hand-build StackItem structs; build neo_vm_rs::StackValue first"
    );
}

#[test]
fn contract_group_projects_through_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let contract_group = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/manifest/contract_group.rs"),
    )
    .unwrap();

    assert!(
        contract_group.contains("use neo_vm_rs::StackValue;"),
        "ContractGroup should import neo_vm_rs::StackValue directly for manifest projection"
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
             neo_vm_rs::StackValue first"
        );
    }
}

#[test]
fn contract_abi_projects_through_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let contract_abi =
        read_source(workspace.join("neo-core/src/smart_contract/manifest/contract_abi.rs"));

    assert!(
        contract_abi.contains("use neo_vm_rs::StackValue;"),
        "ContractAbi should import neo_vm_rs::StackValue directly for manifest projection"
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
             neo_vm_rs::StackValue first"
        );
    }
}

#[test]
fn contract_manifest_and_state_project_through_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let manifest = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/manifest/contract_manifest.rs"),
    )
    .unwrap();
    let contract_state =
        read_source(workspace.join("neo-core/src/smart_contract/contract_state.rs"));

    assert!(
        manifest.contains("use neo_vm_rs::StackValue;"),
        "ContractManifest should import neo_vm_rs::StackValue directly for metadata projection"
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
             build neo_vm_rs::StackValue first"
        );
    }

    assert!(
        contract_state.contains("use neo_vm_rs::StackValue;"),
        "ContractState should import neo_vm_rs::StackValue directly for metadata projection"
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
             build neo_vm_rs::StackValue first"
        );
    }
}

#[test]
fn contract_management_persisted_contract_state_uses_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let source = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/native/contract_management/mod.rs"),
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
        source.contains("use neo_vm_rs::StackValue;")
            || source.contains("use neo_vm_rs::{ExecutionEngineLimits, StackValue};"),
        "ContractManagement persisted ContractState storage should import neo_vm_rs::StackValue"
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
        workspace.join("neo-core/src/smart_contract/native/contract_management/validation.rs"),
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
         neo_vm_rs::StackValue projection"
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

    let transaction_state = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/native/transaction_state.rs"),
    )
    .unwrap();
    assert!(
        transaction_state.contains("use neo_vm_rs::StackValue;")
            || transaction_state.contains("use neo_vm_rs::{StackValue, VmState as VMState};"),
        "TransactionState should import neo_vm_rs::StackValue directly for native data projection"
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
             neo_vm_rs::StackValue first"
        );
    }

    let oracle_request =
        read_source(workspace.join("neo-core/src/smart_contract/native/oracle_request.rs"));
    let oracle_storage = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/native/oracle_contract/storage.rs"),
    )
    .unwrap();
    assert!(
        oracle_request.contains("use neo_vm_rs::StackValue;"),
        "OracleRequest should import neo_vm_rs::StackValue directly for native data projection"
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
             neo_vm_rs::StackValue first"
        );
    }
    let oracle_request_storage_section = oracle_storage
        .split("fn read_id_list")
        .next()
        .expect("Oracle request storage section");
    assert!(
        oracle_request_storage_section.contains("use neo_vm_rs::StackValue;")
            && oracle_request_storage_section.contains("BinarySerializer::serialize_stack_value")
            && oracle_request_storage_section.contains("BinarySerializer::deserialize_stack_value")
            && oracle_request_storage_section.contains("OracleRequest::new(")
            && oracle_request_storage_section.contains(".to_stack_value()"),
        "Oracle request storage should serialize/deserialize direct neo_vm_rs::StackValue \
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
        read_source(workspace.join("neo-core/src/smart_contract/native/trimmed_block.rs"));
    assert!(
        trimmed_block.contains("use neo_vm_rs::StackValue;"),
        "TrimmedBlock should import neo_vm_rs::StackValue directly for native data projection"
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
             neo_vm_rs::StackValue first"
        );
    }

    let deposit_section =
        read_source(workspace.join("neo-core/src/smart_contract/native/notary/deposit.rs"));
    assert!(
        deposit_section.contains("use neo_vm_rs::StackValue;"),
        "Notary Deposit should import neo_vm_rs::StackValue directly for native data projection"
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

    let hash_index_state = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/native/hash_index_state.rs"),
    )
    .unwrap();
    assert!(
        hash_index_state.contains("use neo_vm_rs::StackValue;"),
        "HashIndexState should import neo_vm_rs::StackValue directly for native data projection"
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
        read_source(workspace.join("neo-core/src/smart_contract/native/account_state.rs"));
    let gas_token =
        read_source(workspace.join("neo-core/src/smart_contract/native/gas_token/mod.rs"));
    assert!(
        account_state.contains("use neo_vm_rs::StackValue;"),
        "native AccountState should import neo_vm_rs::StackValue directly for native data \
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
        "GAS account state reads should deserialize directly into neo_vm_rs::StackValue"
    );
    assert!(
        gas_token.contains("BinarySerializer::serialize_stack_value")
            && gas_token.contains("&state.to_stack_value()")
            && !gas_token.contains("&state.to_stack_item()?"),
        "GAS account state writes should serialize direct StackValue projections"
    );

    let policy_contract = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/native/policy_contract/mod.rs"),
    )
    .unwrap();
    let policy_whitelist_section = policy_contract
        .split("/// Whitelisted fee contract info.")
        .nth(1)
        .and_then(|tail| tail.split("/// The Policy native contract.").next())
        .expect("Policy WhitelistedContract section");
    assert!(
        policy_contract.contains("use neo_vm_rs::StackValue;")
            || policy_contract.contains("use neo_vm_rs::{ExecutionEngineLimits, StackValue};"),
        "Policy WhitelistedContract should import neo_vm_rs::StackValue directly for \
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
             {local_builder}; build neo_vm_rs::StackValue first"
        );
    }
}

#[test]
fn policy_whitelist_storage_uses_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let policy_contract = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/native/policy_contract/mod.rs"),
    )
    .unwrap();
    let setters = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/native/policy_contract/setters.rs"),
    )
    .unwrap();
    let snapshot = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/native/policy_contract/snapshot.rs"),
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
    let types =
        read_source(workspace.join("neo-core/src/smart_contract/native/neo_token/types.rs"));
    let native_impl = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/native/neo_token/native_impl.rs"),
    )
    .unwrap();
    let governance = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/native/neo_token/governance.rs"),
    )
    .unwrap();
    let nep17 =
        read_source(workspace.join("neo-core/src/smart_contract/native/neo_token/nep17.rs"));
    let native_account_projection = native_impl
        .split("impl NativeContract for NeoToken")
        .next()
        .expect("NeoAccountState helper section");

    assert!(
        types.contains("use neo_vm_rs::StackValue;"),
        "NeoToken state structs should import neo_vm_rs::StackValue directly for native \
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
        "NeoToken persisted state reads should deserialize directly into neo_vm_rs::StackValue"
    );
    for (name, source) in [
        ("native_impl.rs", native_impl.as_str()),
        ("governance.rs", governance.as_str()),
        ("nep17.rs", nep17.as_str()),
    ] {
        assert!(
            source.contains("BinarySerializer::serialize_stack_value")
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
             {local_builder}; build neo_vm_rs::StackValue first"
        );
    }
}

#[test]
fn neo_token_committee_payloads_use_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let committee =
        read_source(workspace.join("neo-core/src/smart_contract/native/neo_token/committee.rs"));
    let governance =
        read_source(workspace.join("neo-core/src/smart_contract/native/neo_token/governance.rs"));

    assert!(
        committee.contains("BinarySerializer::deserialize_stack_value(&bytes)")
            && committee.contains("decode_committee_stack_value")
            && committee.contains("decode_committee_with_votes_value"),
        "NeoToken committee cache reads should deserialize direct neo_vm_rs::StackValue payloads"
    );
    assert!(
        committee.contains("StackValue::Array")
            && committee.contains("StackValue::Struct")
            && committee.contains("StackValue::ByteString")
            && committee.contains("StackValue::BigInteger")
            && committee.contains("BinarySerializer::serialize_stack_value"),
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
                && section.contains("BinarySerializer::serialize_stack_value"),
            "{method} should serialize its ABI result through neo_vm_rs::StackValue"
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
fn role_management_designated_nodes_storage_uses_neo_vm_rs_stack_value() {
    let workspace = workspace_root();
    let source = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/native/role_management/storage.rs"),
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
        source.contains("use neo_vm_rs::StackValue;"),
        "RoleManagement persisted designated-node payloads should import neo_vm_rs::StackValue"
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
    let script_builder =
        read_source(workspace.join("neo-core/src/script_builder.rs"));

    assert!(
        !script_builder.contains("use crate::neo_vm::script::Script")
            && !script_builder.contains("pub fn to_script")
            && !script_builder.contains("Script::new_relaxed"),
        "ScriptBuilder should return script bytes only; callers that need the local VM \
         runtime can construct neo_vm::Script explicitly at that boundary"
    );
}

#[test]
fn script_builder_does_not_use_local_vm_error_types() {
    let workspace = workspace_root();
    let script_builder =
        read_source(workspace.join("neo-core/src/script_builder.rs"));

    assert!(
        !script_builder.contains("VmError")
            && !script_builder.contains("VmResult")
            && !script_builder.contains("crate::neo_vm::error"),
        "ScriptBuilder is a script-byte construction helper, so fallible builder APIs \
         should use core errors instead of local VM runtime errors"
    );
}

#[test]
fn local_vm_docs_do_not_advertise_deleted_neo_vm_crate() {
    let workspace = workspace_root();
    let vm_dir = workspace.join("neo-vm/src");
    let mut offenders = Vec::new();
    collect_rs_files(&vm_dir, &mut offenders);
    offenders.retain(|path| {
        fs::read_to_string(path)
            .map(|source| {
                source.contains("use neo_vm::")
                    || source.contains("neo_vm::VmResult")
                    || source.contains("Layer 1 (Core):   neo-vm")
                    || source.contains("YOU ARE HERE")
            })
            .unwrap_or(false)
    });

    assert!(
        offenders.is_empty(),
        "local VM docs should not advertise the deleted standalone neo-vm crate or \
         hide direct neo_vm_rs imports: {offenders:?}"
    );
}

#[test]
fn instruction_parsing_uses_neo_vm_rs_opcode_operand_metadata_directly() {
    let workspace = workspace_root();
    let instruction_path = workspace.join("neo-vm/src/instruction.rs");
    let neo_vm_mod = read_source(workspace.join("neo-vm/src/lib.rs"));
    let script = read_source(workspace.join("neo-vm/src/script.rs"));
    let vm_error = read_source(workspace.join("neo-vm/src/error.rs"));
    let parsed = Instruction::parse(&[OpCode::JMP.byte(), 0x10], 0).expect("JMP parses");
    let mut local_instruction_imports = Vec::new();

    for root in [
        workspace.join("neo-core/src"),
        workspace.join("neo-core/tests"),
        workspace.join("neo-rpc/src"),
        workspace.join("neo-rpc/tests"),
    ] {
        let mut files = Vec::new();
        collect_rs_files(&root, &mut files);
        for path in files {
            let relative = path.strip_prefix(&workspace).unwrap().to_string_lossy();
            let normalized_relative = relative.replace('\\', "/");
            let source = read_source(&path);
            if source.contains("crate::neo_vm::instruction::Instruction")
                || source.contains("neo_core::neo_vm::instruction::Instruction")
                || contains_braced_neo_vm_import(&source, "crate::neo_vm::{", "Instruction")
                || contains_braced_neo_vm_import(&source, "neo_core::neo_vm::{", "Instruction")
                || contains_braced_neo_vm_import(&source, "super::{", "Instruction")
            {
                local_instruction_imports.push(normalized_relative);
            }
        }
    }

    assert!(
        !instruction_path.exists()
            && !neo_vm_mod.contains("pub mod instruction")
            && !neo_vm_mod.contains("pub use instruction::Instruction"),
        "neo-core should not keep a local Instruction facade"
    );
    assert!(
        local_instruction_imports.is_empty(),
        "callers should import neo_vm_rs::Instruction directly: {local_instruction_imports:?}"
    );
    assert!(
        script.contains("parse_script_instructions")
            && !script.contains("let mut position = 0")
            && !script.contains("while position < self.script.len()")
            && !script.contains("parse_from_neo_io_reader")
            && !script.contains("parse_from_reader"),
        "local Script should delegate bulk bytecode parsing and validation to \
         neo-vm-rs instead of keeping local instruction-walking loops"
    );
    assert!(
        vm_error.contains("impl From<neo_vm_rs::InstructionError> for VmError"),
        "neo-core should map shared Instruction errors at the VM boundary"
    );
    assert!(
        parsed.opcode() == OpCode::JMP && parsed.size() == 2 && parsed.operand_as::<i8>() == Ok(16),
        "neo-vm-rs Instruction should expose the operand decoding used by local dispatch"
    );
}

#[test]
fn p2p_validation_uses_direct_neo_vm_rs_script_validation() {
    let workspace = workspace_root();
    for relative in [
        "neo-core/src/network/p2p/payloads/header/verification.rs",
        "neo-core/src/network/p2p/payloads/transaction/verification.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        assert!(
            source.contains("validate_strict_script"),
            "{relative} should validate script bytecode through the direct neo-vm-rs \
             script validator"
        );
        assert!(
            !source.contains("crate::neo_vm::Script::new")
                && !source.contains("neo_vm::Script::new"),
            "{relative} should not construct the local neo_vm::Script just to validate \
             P2P script bytecode"
        );
    }

    let validator = read_source(workspace.join("neo-core/src/script_validation.rs"));
    assert!(
        validator.contains("pub use neo_vm_rs::{")
            && validator.contains("parse_script_instructions")
            && validator.contains("validate_script")
            && validator.contains("validate_strict_script")
            && validator.contains("ScriptInstruction")
            && validator.contains("ValidatedScript")
            && !validator.contains("opcode.operand_size()")
            && !validator.contains("opcode.operand_prefix()")
            && !validator.contains("Instruction::parse(script, position)"),
        "neo-core script_validation should re-export neo-vm-rs validation instead of \
         duplicating instruction parsing or opcode operand metadata"
    );
}

#[test]
fn contract_management_uses_direct_script_validation_for_nef_abi_checks() {
    let workspace = workspace_root();
    for relative in [
        "neo-core/src/smart_contract/native/contract_management/deploy.rs",
        "neo-core/src/smart_contract/native/contract_management/update.rs",
        "neo-core/src/smart_contract/native/contract_management/validation.rs",
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
        module_docs.contains("Opcode metadata and ABI-level semantics are imported directly from `neo-vm-rs`"),
        "neo-core::neo_vm docs should state that canonical opcode metadata and ABI semantics live in neo-vm-rs"
    );
}

#[test]
fn newbuffer_reuses_neo_vm_rs_collection_semantics() {
    let workspace = workspace_root();
    let splice =
        read_source(workspace.join("neo-vm/src/jump_table/splice.rs"));

    assert!(
        splice.contains("neo_vm_rs::semantics::collections::new_buffer"),
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
    let compound =
        read_source(workspace.join("neo-vm/src/jump_table/compound.rs"));

    assert!(
        compound.contains("neo_vm_rs::semantics::collections::new_array("),
        "NEWARRAY should reuse neo-vm-rs collection semantics instead of rebuilding \
         null-filled Array values locally"
    );
    assert!(
        compound.contains("neo_vm_rs::semantics::collections::new_array_t"),
        "NEWARRAY_T should reuse neo-vm-rs typed-array default semantics"
    );
    assert!(
        compound.contains("neo_vm_rs::semantics::collections::new_struct"),
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
    let compound =
        read_source(workspace.join("neo-vm/src/jump_table/compound.rs"));

    assert!(
        compound.contains("neo_vm_rs::semantics::collections::pack_map(Vec::new())"),
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
    let stack_item =
        read_source(workspace.join("neo-vm/src/stack_item/stack_item.rs"));

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
    let stack_item =
        read_source(workspace.join("neo-vm/src/stack_item/stack_item.rs"));

    assert!(
        stack_item.contains("neo_vm_rs::semantics::comparison::boolean_value"),
        "StackItem primitive truthiness should reuse neo-vm-rs StackValue truthiness rules"
    );
    assert!(
        stack_item.contains("Cannot convert ByteString to Boolean")
            && stack_item.contains("b.iter().any(|byte| *byte != 0)"),
        "ByteString truthiness must preserve the local max-size guard and then compute the \
         neo-vm-rs NotZero rule (true iff any byte is non-zero), matching C# Unsafe.NotZero"
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
    let stack_item =
        read_source(workspace.join("neo-vm/src/stack_item/stack_item.rs"));

    assert!(
        stack_item.contains("neo_vm_rs::semantics::conversion::convert_value"),
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
    let stack_item =
        read_source(workspace.join("neo-vm/src/stack_item/stack_item.rs"));

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
fn stack_item_type_facade_is_removed_and_byte_tags_use_neo_vm_rs() {
    let workspace = workspace_root();
    let stack_item_type_path = workspace.join("neo-vm/src/stack_item/stack_item_type.rs");
    let stack_item_mod =
        read_source(workspace.join("neo-vm/src/stack_item/mod.rs"));
    let neo_vm_mod = read_source(workspace.join("neo-vm/src/lib.rs"));
    let serializer =
        read_source(workspace.join("neo-core/src/smart_contract/binary_serializer.rs"));
    let stack_item =
        read_source(workspace.join("neo-vm/src/stack_item/stack_item.rs"));

    assert!(
        !stack_item_type_path.exists(),
        "neo-rs should not keep a local StackItemType facade file"
    );
    assert!(
        !stack_item_mod.contains("pub mod stack_item_type")
            && !stack_item_mod.contains("pub use stack_item_type::StackItemType"),
        "neo-rs stack_item module should not expose a local StackItemType facade"
    );
    assert!(
        !neo_vm_mod.contains("StackItemType"),
        "neo_vm should not re-export StackItemType; callers should import neo_vm_rs::StackItemType"
    );
    for cast in [
        "StackItemType::Any as u8",
        "StackItemType::Boolean as u8",
        "StackItemType::Integer as u8",
        "StackItemType::ByteString as u8",
        "StackItemType::Buffer as u8",
        "StackItemType::Array as u8",
        "StackItemType::Struct as u8",
        "StackItemType::Map as u8",
    ] {
        assert!(
            !serializer.contains(cast),
            "BinarySerializer should write StackItemType tags through to_byte(), not {cast}"
        );
    }
    assert!(
        !stack_item.contains("stack_item_type() as u8"),
        "StackItem ordering should compare StackItemType::to_byte() values instead of \
         relying on enum casts"
    );
}

#[test]
fn size_primitive_lengths_reuse_neo_vm_rs_byte_string_rules() {
    let workspace = workspace_root();
    let compound =
        read_source(workspace.join("neo-vm/src/jump_table/compound.rs"));

    assert!(
        compound.contains("neo_vm_rs::semantics::collections::size(&value)"),
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
    let numeric =
        read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

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
    let numeric =
        read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

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
    let numeric =
        read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

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
    let numeric =
        read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

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
    let numeric =
        read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

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
    let numeric =
        read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

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
    let numeric =
        read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

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
    let numeric =
        read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

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
    let numeric =
        read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

    for helper in ["shl_value", "shr_value"] {
        assert!(
            numeric.contains(&format!("shift(engine, arithmetic::{helper}")),
            "SHL/SHR should reuse neo-vm-rs StackValue helper {helper}"
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
    let numeric =
        read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

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
    let numeric =
        read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

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
    let numeric =
        read_source(workspace.join("neo-vm/src/jump_table/numeric.rs"));

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
    let bitwise =
        read_source(workspace.join("neo-vm/src/jump_table/bitwisee.rs"));

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
        types.contains("neo_vm_rs::semantics::comparison::is_null"),
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
    let types =
        read_source(workspace.join("neo-vm/src/jump_table/types.rs"));

    assert!(
        types.contains("neo_vm_rs::semantics::conversion::is_type"),
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
        stack_item.contains("neo_vm_rs::semantics::conversion::convert_value"),
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
    let splice =
        read_source(workspace.join("neo-vm/src/jump_table/splice.rs"));

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
    let splice =
        read_source(workspace.join("neo-vm/src/jump_table/splice.rs"));

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
    let compound =
        read_source(workspace.join("neo-vm/src/jump_table/compound.rs"));

    assert!(
        compound.contains("neo_vm_rs::semantics::collections::has_key"),
        "HASKEY for byte sequences should reuse neo-vm-rs collection semantics after \
         local index adaptation"
    );
    assert!(
        compound.contains("neo_vm_rs::semantics::collections::pick_item"),
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
fn historical_vm_bug_fixes_stay_guarded_at_neo_vm_rs_boundary() {
    let workspace = workspace_root();
    let root_manifest = fs::read_to_string(workspace.join("Cargo.toml")).unwrap();
    let compound =
        read_source(workspace.join("neo-vm/src/jump_table/compound.rs"));
    let manifest = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/manifest/contract_manifest.rs"),
    )
    .unwrap();
    let oracle_contract =
        read_source(workspace.join("neo-core/src/smart_contract/native/oracle_contract.rs"));
    let oracle_storage = read_source(
        workspace.join("neo-core/src/smart_contract/native/oracle_contract/storage.rs"),
    );
    let oracle_post_persist = read_source(
        workspace.join("neo-core/src/smart_contract/native/oracle_contract/post_persist.rs"),
    );
    let oracle_pricing = read_source(
        workspace.join("neo-core/src/smart_contract/native/oracle_contract/pricing.rs"),
    );
    let oracle_request = read_source(
        workspace.join("neo-core/src/smart_contract/native/oracle_contract/request.rs"),
    );
    let oracle_contract_sources =
        format!("{oracle_contract}\n{oracle_storage}\n{oracle_post_persist}\n{oracle_pricing}\n{oracle_request}");
    let notary =
        read_source(workspace.join("neo-core/src/smart_contract/native/notary.rs"));
    let notary_native_impl =
        read_source(workspace.join("neo-core/src/smart_contract/native/notary/native_impl.rs"));
    let notary_sources = format!("{notary}\n{notary_native_impl}");
    let contract_update = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/native/contract_management/update.rs"),
    )
    .unwrap();
    let json_serializer =
        read_source(workspace.join("neo-core/src/smart_contract/json_serializer.rs"));
    let transaction_verification = fs::read_to_string(
        workspace.join("neo-core/src/network/p2p/payloads/transaction/verification.rs"),
    )
    .unwrap();
    let block_verification =
        read_source(workspace.join("neo-core/src/network/p2p/payloads/block/verification.rs"));

    assert!(
        root_manifest.contains("neo-vm-rs = { path = \"../neo-vm-rs\" }"),
        "neo-rs should consume the sibling neo-vm-rs crate while this cross-repo VM \
         replacement is in progress"
    );

    // de5acd8f: ByteString/Buffer integer conversion must be signed little-endian
    // two's-complement, not sign-magnitude.
    assert_eq!(StackValue::ByteString(vec![0xc9]).to_i128(), Some(-55));
    assert_eq!(
        StackValue::ByteString(vec![1, 0, 0, 0x80]).to_i128(),
        Some(-2_147_483_647)
    );

    // b4f8bbb6: VM EQUAL is type-strict for primitives; byte-identical
    // Integer/ByteString values must not compare equal.
    assert!(neo_vm_rs::semantics::comparison::not_equal_values(
        &StackValue::Integer(1),
        &StackValue::ByteString(vec![1])
    ));

    // b4f8bbb6 / 8be8f005 / bug #11: minimal signed-LE integer bytes matter
    // for persisted payloads and PrimitiveType.Size.
    assert_eq!(neo_vm_rs::encode_integer(0), Vec::<u8>::new());
    assert_eq!(neo_vm_rs::encode_integer(128), vec![0x80, 0x00]);
    assert_eq!(
        StackValue::Boolean(false).to_byte_string_bytes(),
        Some(vec![0]),
        "C# Boolean.Memory is one byte even for false"
    );

    // The sibling neo-vm-rs public ABI collection helpers cover the primitive
    // SIZE/PICKITEM cases from bugs #4/#11, so neo-core can route those paths
    // through shared semantics instead of keeping a separate local table.
    assert_eq!(
        neo_vm_rs::semantics::collections::size(&StackValue::Integer(128)),
        Ok(2)
    );
    assert_eq!(
        neo_vm_rs::semantics::collections::size(&StackValue::Boolean(false)),
        Ok(1)
    );
    assert_eq!(
        neo_vm_rs::semantics::collections::pick_item(
            &StackValue::Integer(128),
            &StackValue::Integer(0)
        ),
        Ok(StackValue::Integer(128))
    );
    assert_eq!(
        neo_vm_rs::semantics::collections::pick_item(
            &StackValue::Integer(128),
            &StackValue::Integer(-1)
        ),
        Err("PICKITEM: byte index out of range".to_string())
    );
    assert_eq!(
        neo_vm_rs::semantics::collections::pick_item(
            &StackValue::Integer(128),
            &StackValue::Integer(2)
        ),
        Err("PICKITEM: byte index out of range".to_string())
    );
    assert_eq!(
        neo_vm_rs::semantics::collections::pick_item(
            &StackValue::Boolean(false),
            &StackValue::Integer(0)
        ),
        Ok(StackValue::Integer(0))
    );
    assert_eq!(
        neo_vm_rs::semantics::collections::pick_item(
            &StackValue::Boolean(false),
            &StackValue::Integer(-1)
        ),
        Err("PICKITEM: byte index out of range".to_string())
    );
    assert_eq!(
        neo_vm_rs::semantics::collections::pick_item(
            &StackValue::Boolean(false),
            &StackValue::Integer(1)
        ),
        Err("PICKITEM: byte index out of range".to_string())
    );
    assert!(
        compound.contains("neo_vm_rs::semantics::collections::size"),
        "SIZE should use neo-vm-rs public collection semantics now that it supports \
         Integer and Boolean PrimitiveType.Size"
    );
    assert!(
        !compound.contains("stack_value_byte_string_len"),
        "SIZE should not keep a second local primitive byte-length helper"
    );
    assert!(
        compound.contains("item @ (StackItem::Integer(_) | StackItem::Boolean(_))")
            && compound.contains("neo_vm_rs::StackValue::try_from(item)?")
            && compound.contains("neo_vm_rs::semantics::collections::pick_item"),
        "PICKITEM should route primitive Boolean/Integer spans through neo-vm-rs \
         collection semantics"
    );
    assert!(
        !compound.contains("if b { vec![1] } else { Vec::new() }"),
        "PICKITEM must not treat Boolean false as an empty span; C# Boolean.Memory is [0]"
    );

    let mut size_int = ScriptBuilder::new();
    size_int.emit_push_int(128);
    size_int.emit_opcode(OpCode::SIZE);
    size_int.emit_opcode(OpCode::RET);
    assert_eq!(execute_int(size_int), BigInt::from(2));

    let mut size_bool = ScriptBuilder::new();
    size_bool.emit_push_bool(true);
    size_bool.emit_opcode(OpCode::SIZE);
    size_bool.emit_opcode(OpCode::RET);
    assert_eq!(execute_int(size_bool), BigInt::from(1));

    let mut pick_int = ScriptBuilder::new();
    pick_int.emit_push_int(128);
    pick_int.emit_push_int(0);
    pick_int.emit_opcode(OpCode::PICKITEM);
    pick_int.emit_opcode(OpCode::RET);
    assert_eq!(execute_int(pick_int), BigInt::from(128));

    let mut pick_bool = ScriptBuilder::new();
    pick_bool.emit_push_bool(true);
    pick_bool.emit_push_int(0);
    pick_bool.emit_opcode(OpCode::PICKITEM);
    pick_bool.emit_opcode(OpCode::RET);
    assert_eq!(execute_int(pick_bool), BigInt::from(1));

    let mut pick_false = ScriptBuilder::new();
    pick_false.emit_push_bool(false);
    pick_false.emit_push_int(0);
    pick_false.emit_opcode(OpCode::PICKITEM);
    pick_false.emit_opcode(OpCode::RET);
    assert_eq!(execute_int(pick_false), BigInt::from(0));

    let mut pick_int_negative = ScriptBuilder::new();
    pick_int_negative.emit_push_int(128);
    pick_int_negative.emit_push_int(-1);
    pick_int_negative.emit_opcode(OpCode::PICKITEM);
    pick_int_negative.emit_opcode(OpCode::RET);
    assert_eq!(execute_state(pick_int_negative), VMState::FAULT);

    let mut pick_int_out_of_range = ScriptBuilder::new();
    pick_int_out_of_range.emit_push_int(128);
    pick_int_out_of_range.emit_push_int(2);
    pick_int_out_of_range.emit_opcode(OpCode::PICKITEM);
    pick_int_out_of_range.emit_opcode(OpCode::RET);
    assert_eq!(execute_state(pick_int_out_of_range), VMState::FAULT);

    let mut pick_false_negative = ScriptBuilder::new();
    pick_false_negative.emit_push_bool(false);
    pick_false_negative.emit_push_int(-1);
    pick_false_negative.emit_opcode(OpCode::PICKITEM);
    pick_false_negative.emit_opcode(OpCode::RET);
    assert_eq!(execute_state(pick_false_negative), VMState::FAULT);

    // Bug #10: ContractManifest StackValue projection must keep the same
    // C# JavaScriptEncoder.Default escaping as the old StackItem path.
    assert!(
        manifest.contains("JsonSerializer::encode_value_csharp_compatible"),
        "ContractManifest StackValue projection must preserve bug #10 C# JSON escaping"
    );
    assert!(
        manifest.contains("\\\\u0026"),
        "ContractManifest tests must keep the block 1,208,916 ampersand escape guard"
    );

    // b4f8bbb6: non-VM native parity fixes are not neo-vm-rs behavior, so keep
    // source-level tripwires where neo-core owns the boundary.
    assert!(
        oracle_contract_sources.contains("BigInt::from_signed_bytes_le(&bytes).to_i64()")
            && oracle_contract_sources
                .contains("StorageItem::from_bytes(BigInt::from(price).to_signed_bytes_le())")
            && oracle_contract_sources.contains("BigInt::from_signed_bytes_le(&args[4])")
            && oracle_contract_sources.contains("BigInt::from_signed_bytes_le(&bytes).to_u64()"),
        "Oracle price/request-id/gas integer paths must stay on C# signed little-endian \
         BigInteger encoding"
    );
    assert!(
        oracle_contract_sources.contains("NativeArgNullMask")
            && oracle_contract_sources.contains("let filter = if filter_was_null")
            && oracle_contract_sources
                .contains("Some(\n                String::from_utf8(args[1].clone())"),
        "Oracle request filter storage must distinguish StackItem.Null from ByteString(\"\")"
    );
    assert!(
        notary_sources.contains("BigInt::from_signed_bytes_le(&data)")
            && notary_sources.contains("BigInt::from(value).to_signed_bytes_le()")
            && notary_sources
                .contains("BigInt::from(DEFAULT_MAX_NOT_VALID_BEFORE_DELTA).to_signed_bytes_le()"),
        "Notary maxDelta storage must stay on C# signed little-endian BigInteger encoding"
    );
    assert!(
        contract_update.contains("engine.put_contract_cache(contract_hash, contract.clone());"),
        "ContractManagement.update must refresh the per-tx contract cache before _deploy"
    );
    assert!(
        root_manifest
            .contains("serde_json = { version = \"1.0\", features = [\"preserve_order\"] }")
            && json_serializer.contains("let mut entries = Vec::with_capacity(obj.len());")
            && json_serializer.contains("MapItem::new_untracked(entries)")
            && !json_serializer.contains("use std::collections::{BTreeMap")
            && !json_serializer.contains("let mut map = BTreeMap::new()"),
        "JsonSerializer.deserialize must preserve JSON object insertion order"
    );
    assert!(
        oracle_contract_sources
            .contains("get_designated_by_role_at(snapshot, Role::Oracle, index)"),
        "Oracle post-persist rewards must use RoleManagement's typed BinarySerializer decoder"
    );
    assert!(
        transaction_verification.contains("pub fn verify_state_dependent_at_height(")
            && transaction_verification.contains("self.verify_state_dependent_at_height("),
        "Transaction verification must expose explicit-height state-dependent verify to avoid \
         current_index=0 fast-sync expiry regressions on the mempool/relay path"
    );
    // C# parity: Block.Verify is defined as `return Header.Verify(...)`. Block import
    // must NOT re-verify each transaction (the header's consensus witness is the
    // integrity guarantee; transactions are executed during persistence, not
    // re-verified at import). Guard against reintroducing per-tx verification here.
    assert!(
        block_verification.contains("self.header.verify(")
            && !block_verification.contains("verify_state_dependent_at_height(")
            && !block_verification.contains("verify_state_independent("),
        "Block::verify must be header-only (matching C# Block.Verify => Header.Verify), not \
         re-verify per-transaction state"
    );
}

#[test]
fn static_syscall_hash_expectations_use_neo_vm_rs_directly() {
    let workspace = workspace_root();
    let parity_tests =
        read_source(workspace.join("neo-core/tests/contract_script_parity_tests.rs"));

    assert!(
        parity_tests.contains("neo_vm_rs::interop_hash(\"System.Crypto.CheckSig\")"),
        "static CheckSig expectation should use neo_vm_rs::interop_hash directly"
    );
    assert!(
        parity_tests.contains("neo_vm_rs::interop_hash(\"System.Crypto.CheckMultisig\")"),
        "static CheckMultisig expectation should use neo_vm_rs::interop_hash directly"
    );
    assert!(
        !parity_tests.contains("ScriptBuilder::hash_syscall(\"System.Crypto."),
        "static test syscall hashes should not route through ScriptBuilder validation"
    );
}

#[test]
fn neo_rpc_uses_neo_vm_rs_opcode_directly() {
    let workspace = workspace_root();
    let rpc_manifest = fs::read_to_string(workspace.join("neo-rpc/Cargo.toml")).unwrap();
    assert!(
        rpc_manifest.contains("neo-vm-rs"),
        "neo-rpc should depend on neo-vm-rs when it needs OpCode"
    );

    let rpc_dir = workspace.join("neo-rpc/src");
    let mut offenders = Vec::new();
    collect_rs_files(&rpc_dir, &mut offenders);
    offenders.retain(|path| {
        fs::read_to_string(path)
            .map(|source| source.contains("neo_core::neo_vm::op_code::OpCode"))
            .unwrap_or(false)
    });

    assert!(
        offenders.is_empty(),
        "neo-rpc should import neo_vm_rs::OpCode directly instead of routing \
         through neo-core's compatibility module: {offenders:?}"
    );
}

#[test]
fn external_crates_import_opcode_from_neo_vm_rs() {
    let workspace = workspace_root();
    for manifest in [
        "neo-consensus/Cargo.toml",
        "benches-package/Cargo.toml",
        "tests/Cargo.toml",
    ] {
        let manifest_text = fs::read_to_string(workspace.join(manifest)).unwrap();
        assert!(
            manifest_text.contains("neo-vm-rs"),
            "{manifest} should declare neo-vm-rs when it uses OpCode"
        );
    }

    let mut offenders = Vec::new();
    for dir in [
        "neo-consensus/src",
        "benches-package/benches",
        "tests/tests",
        "neo-core/tests",
        "neo-core/examples",
    ] {
        collect_rs_files(&workspace.join(dir), &mut offenders);
    }
    offenders.retain(|path| {
        if path
            .file_name()
            .is_some_and(|name| name == "no_local_neo_vm_dependency.rs")
        {
            return false;
        }

        fs::read_to_string(path)
            .map(|source| {
                source.contains("op_code::OpCode")
                    || source.contains("neo_core::neo_vm::OpCode")
                    || contains_neo_core_opcode_import(&source)
            })
            .unwrap_or(false)
    });

    assert!(
        offenders.is_empty(),
        "external crates/tests/benches should import neo_vm_rs::OpCode directly: {offenders:?}"
    );
}

#[test]
fn workspace_tests_and_benches_do_not_import_local_vm_runtime() {
    let workspace = workspace_root();
    let mut offenders = Vec::new();
    for dir in ["benches-package/benches", "tests/tests"] {
        collect_rs_files(&workspace.join(dir), &mut offenders);
    }

    offenders.retain(|path| {
        if path
            .file_name()
            .is_some_and(|name| name == "no_local_neo_vm_dependency.rs")
        {
            return false;
        }

        fs::read_to_string(path)
            .map(|source| {
                source.contains("neo_core::neo_vm")
                    || source.contains("crate::neo_vm")
                    || source.contains("use neo_vm::")
            })
            .unwrap_or(false)
    });

    assert!(
        offenders.is_empty(),
        "workspace-level tests and benchmarks should use neo_vm_rs directly instead of \
         local neo_core::neo_vm runtime APIs: {offenders:?}"
    );
}

#[test]
fn smart_contract_support_modules_use_vm_runtime_boundary() {
    let workspace = workspace_root();
    for relative in [
        "neo-core/src/smart_contract/diagnostic.rs",
        "neo-core/src/smart_contract/execution_context_state.rs",
        "neo-core/src/smart_contract/application_engine_iterator.rs",
        "neo-core/src/smart_contract/iterators/iterator_interop.rs",
        "neo-core/src/smart_contract/iterators/storage_iterator.rs",
        "neo-core/src/smart_contract/storage_context.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        assert!(
            source.contains("crate::vm_runtime"),
            "{relative} should import host-runtime-only local VM types through vm_runtime"
        );
        assert!(
            !source.contains("crate::neo_vm"),
            "{relative} should not import host-runtime-only local VM types through the \
             neo_vm implementation tree"
        );
    }
}

#[test]
fn protocol_data_modules_use_vm_runtime_stack_items() {
    let workspace = workspace_root();
    for relative in [
        "neo-application-logs/src/service.rs",
        "neo-core/src/ledger/blockchain_application_executed.rs",
        "neo-core/src/network/p2p/payloads/signer.rs",
        "neo-core/src/network/p2p/payloads/transaction/mod.rs",
        "neo-tokens-tracker/src/trackers/tracker_base.rs",
        "neo-tokens-tracker/src/trackers/nep_17/nep17_tracker.rs",
        "neo-tokens-tracker/src/trackers/nep_11/nep11_tracker.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        // The neo_vm host StackItem must come through the `neo_vm` seam — either
        // `crate::neo_vm::StackItem` (inside neo-core) or `neo_core::neo_vm::StackItem`
        // (extracted plugin crates). `neo_vm::StackItem` matches both and excludes
        // the pure-VM `neo_vm_rs::StackItem`.
        assert!(
            source.contains("neo_vm::StackItem"),
            "{relative} should import StackItem through the neo_vm host seam"
        );
    }
}

#[test]
fn manifest_modules_use_vm_runtime_stack_items() {
    let workspace = workspace_root();
    for relative in [
        "neo-core/src/smart_contract/manifest/contract_abi.rs",
        "neo-core/src/smart_contract/manifest/contract_event_descriptor.rs",
        "neo-core/src/smart_contract/manifest/contract_group.rs",
        "neo-core/src/smart_contract/manifest/contract_manifest.rs",
        "neo-core/src/smart_contract/manifest/contract_method_descriptor.rs",
        "neo-core/src/smart_contract/manifest/contract_parameter_definition.rs",
        "neo-core/src/smart_contract/manifest/contract_permission.rs",
        "neo-core/src/smart_contract/manifest/contract_permission_descriptor.rs",
        "neo-core/src/smart_contract/manifest/wild_card_container.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        // The neo_vm host StackItem must come through the `neo_vm` seam — either
        // `crate::neo_vm::StackItem` (inside neo-core) or `neo_core::neo_vm::StackItem`
        // (extracted plugin crates). `neo_vm::StackItem` matches both and excludes
        // the pure-VM `neo_vm_rs::StackItem`.
        assert!(
            source.contains("neo_vm::StackItem"),
            "{relative} should import StackItem through the neo_vm host seam"
        );
    }
}

#[test]
fn native_event_and_data_modules_use_vm_runtime_stack_items() {
    let workspace = workspace_root();
    for relative in [
        "neo-core/src/smart_contract/contract_state.rs",
        "neo-core/src/smart_contract/notify_event_args.rs",
        "neo-core/src/smart_contract/native/oracle_contract/events.rs",
        "neo-core/src/smart_contract/native/trimmed_block.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        // The neo_vm host StackItem must come through the `neo_vm` seam — either
        // `crate::neo_vm::StackItem` (inside neo-core) or `neo_core::neo_vm::StackItem`
        // (extracted plugin crates). `neo_vm::StackItem` matches both and excludes
        // the pure-VM `neo_vm_rs::StackItem`.
        assert!(
            source.contains("neo_vm::StackItem"),
            "{relative} should import StackItem through the neo_vm host seam"
        );
    }
}

#[test]
fn host_adapter_modules_use_vm_runtime_stack_items() {
    let workspace = workspace_root();
    for relative in [
        "neo-core/src/smart_contract/interoperable.rs",
        "neo-core/src/witness_rule/stack_projection.rs",
        "neo-core/src/neo_system/persistence.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        // The neo_vm host StackItem must come through the `neo_vm` seam — either
        // `crate::neo_vm::StackItem` (inside neo-core) or `neo_core::neo_vm::StackItem`
        // (extracted plugin crates). `neo_vm::StackItem` matches both and excludes
        // the pure-VM `neo_vm_rs::StackItem`.
        assert!(
            source.contains("neo_vm::StackItem"),
            "{relative} should import StackItem through the neo_vm host seam"
        );
    }
}

#[test]
fn native_contract_modules_use_vm_runtime_stack_items() {
    let workspace = workspace_root();
    for relative in [
        "neo-core/src/smart_contract/native/contract_management/mod.rs",
        "neo-core/src/smart_contract/native/contract_management/tests.rs",
        "neo-core/src/smart_contract/native/fungible_token.rs",
        "neo-core/src/smart_contract/native/gas_token/mod.rs",
        "neo-core/src/smart_contract/native/interoperable_list.rs",
        "neo-core/src/smart_contract/native/ledger_contract/native_impl.rs",
        "neo-core/src/smart_contract/native/neo_token/mod.rs",
        "neo-core/src/smart_contract/native/notary.rs",
        "neo-core/src/smart_contract/native/oracle_contract/response.rs",
        "neo-core/src/smart_contract/native/oracle_contract/storage.rs",
        "neo-core/src/smart_contract/native/policy_contract/mod.rs",
        "neo-core/src/smart_contract/native/policy_contract/tests.rs",
        "neo-core/src/smart_contract/native/role_management.rs",
        "neo-core/src/smart_contract/native/std_lib/helpers.rs",
        "neo-core/src/smart_contract/native/std_lib/strings.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        // The neo_vm host StackItem must come through the `neo_vm` seam — either
        // `crate::neo_vm::StackItem` (inside neo-core) or `neo_core::neo_vm::StackItem`
        // (extracted plugin crates). `neo_vm::StackItem` matches both and excludes
        // the pure-VM `neo_vm_rs::StackItem`.
        assert!(
            source.contains("neo_vm::StackItem"),
            "{relative} should import StackItem through the neo_vm host seam"
        );
    }
}

#[test]
fn fuzz_script_parser_uses_direct_script_validation() {
    let workspace = workspace_root();
    let target =
        read_source(workspace.join("fuzz/fuzz_targets/fuzz_script_parse.rs"));

    assert!(
        target.contains("neo_core::script_validation::validate_script"),
        "script parsing fuzz target should exercise the direct neo-vm-rs-backed bytecode \
         validator"
    );
    assert!(
        !target.contains("neo_core::neo_vm::Script")
            && !target.contains("use neo_vm::")
            && !target.contains("Script::new")
            && !target.contains("Script::new_relaxed"),
        "script parsing fuzz target should not construct the local neo_vm::Script"
    );
}

#[test]
fn fuzz_docs_describe_direct_script_validation_target() {
    let workspace = workspace_root();
    let readme = fs::read_to_string(workspace.join("fuzz/README.md")).unwrap();

    assert!(
        readme.contains("neo_core::script_validation::validate_script"),
        "fuzz README should document the direct neo-vm-rs-backed script validation target"
    );
    assert!(
        !readme.contains("neo_vm::Script"),
        "fuzz README should not advertise the deleted standalone neo_vm::Script target"
    );
}

#[test]
fn script_disassembly_examples_use_direct_script_validation() {
    let workspace = workspace_root();
    for relative in [
        "neo-core/examples/disassemble_contract_script.rs",
        "neo-core/examples/disassemble_tx.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        assert!(
            source.contains("script_validation") && source.contains("parse_script_instructions"),
            "{relative} should decode bytecode through the direct neo-vm-rs-backed \
             script parser"
        );
        assert!(
            !source.contains("neo_core::neo_vm::Script")
                && !source.contains("Script::new")
                && !source.contains("get_instruction"),
            "{relative} should not construct local neo_vm::Script for disassembly"
        );
    }
}

#[test]
fn storage_inspection_examples_use_neo_vm_rs_stackvalue_boundary() {
    let workspace = workspace_root();
    let serializer =
        read_source(workspace.join("neo-core/src/smart_contract/binary_serializer.rs"));
    assert!(
        serializer.contains("pub fn deserialize_stack_value")
            && serializer.contains("neo_vm_rs::StackValue"),
        "BinarySerializer should expose a neo-vm-rs StackValue deserialization boundary for \
         non-runtime storage inspectors"
    );

    for relative in [
        "neo-core/examples/print_storage_key.rs",
        "neo-core/examples/inspect_neo_account_state.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        assert!(
            source.contains("deserialize_stack_value")
                && source.contains("use neo_vm_rs::StackValue"),
            "{relative} should deserialize storage payloads directly into \
             neo_vm_rs::StackValue"
        );
        assert!(
            !source.contains("neo_core::neo_vm")
                && !source.contains("ExecutionEngineLimits")
                && !source.contains("StackItem"),
            "{relative} should not import local VM runtime stack types for read-only \
             storage inspection"
        );
    }

    let contract_inspector =
        read_source(workspace.join("neo-core/examples/inspect_contract_state.rs"));
    assert!(
        !contract_inspector.contains("neo_core::neo_vm")
            && !contract_inspector.contains("ExecutionEngineLimits")
            && !contract_inspector.contains("BinarySerializer::deserialize"),
        "inspect_contract_state should use direct contract-state deserialization and avoid \
         local VM stack decoding"
    );
}

#[test]
fn replay_debug_example_uses_direct_script_validation_for_instruction_windows() {
    let workspace = workspace_root();
    let source = read_source(workspace.join("neo-core/examples/replay_tx_once.rs"));

    assert!(
        source.contains("script_validation::parse_script_instructions"),
        "replay_tx_once should decode diagnostic instruction windows through the \
         neo-vm-rs-backed script parser"
    );
    assert!(
        !source.contains("neo_core::neo_vm::execution_context::ExecutionContext"),
        "replay_tx_once should not name the local ExecutionContext type just to print \
         script instruction windows"
    );
}

#[test]
fn primitive_interop_validation_uses_stackvalue_directly() {
    let workspace = workspace_root();
    for relative in [
        "neo-core/src/smart_contract/validator_attribute.rs",
        "neo-core/src/smart_contract/max_length_attribute.rs",
        "neo-core/src/smart_contract/interop_parameter_descriptor.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        assert!(
            source.contains("neo_vm_rs::StackValue"),
            "{relative} should validate primitive interop payloads with \
             neo_vm_rs::StackValue"
        );
        assert!(
            !source.contains("crate::neo_vm::StackItem"),
            "{relative} should not import local StackItem for primitive interop validation"
        );
    }

    let max_length =
        read_source(workspace.join("neo-core/src/smart_contract/max_length_attribute.rs"));
    assert!(
        max_length.contains("to_byte_string_bytes()"),
        "MaxLengthAttribute should use neo-vm-rs byte-string conversion rules for \
         primitive integer and boolean lengths"
    );

    let interop = fs::read_to_string(
        workspace.join("neo-core/src/smart_contract/interop_parameter_descriptor.rs"),
    )
    .unwrap();
    assert!(
        interop.contains("ConvertedValue::StackValue")
            && !interop.contains("ConvertedValue::StackItem"),
        "InteropParameterDescriptor should preserve generic values as \
         neo_vm_rs::StackValue, not local StackItem"
    );
}

#[test]
fn script_building_callers_do_not_import_builder_from_neo_vm_facade() {
    let workspace = workspace_root();
    let mut offenders = Vec::new();
    for dir in [
        "neo-core/src",
        "neo-rpc/src",
        "neo-node/src",
        "neo-consensus/src",
        "tests/tests",
        "neo-core/tests",
    ] {
        collect_rs_files(&workspace.join(dir), &mut offenders);
    }
    offenders.retain(|path| {
        if path
            .file_name()
            .is_some_and(|name| name == "no_local_neo_vm_dependency.rs")
        {
            return false;
        }
        if path
            .strip_prefix(workspace.join("neo-vm/src"))
            .is_ok()
        {
            return false;
        }
        if path == &workspace.join("neo-core/src/lib.rs") {
            return false;
        }

        fs::read_to_string(path)
            .map(|source| contains_neo_vm_script_builder_import(&source))
            .unwrap_or(false)
    });

    assert!(
        offenders.is_empty(),
        "script-building callers should import neo_core::script_builder::ScriptBuilder, \
         not route through the local neo_vm facade: {offenders:?}"
    );
}

#[test]
fn script_builder_implementation_lives_outside_local_neo_vm_tree() {
    let workspace = workspace_root();
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));

    assert!(
        workspace.join("neo-core/src/script_builder.rs").exists(),
        "ScriptBuilder should live at neo-core/src/script_builder.rs so script construction \
         is layered outside the local VM runtime"
    );
    assert!(
        !workspace
            .join("neo-vm/src/script_builder.rs")
            .exists(),
        "ScriptBuilder implementation should be moved out of neo-vm/src; keep \
         only a thin compatibility shim if legacy VM internals still need that path"
    );
    assert!(
        !vm_module.contains("pub mod script_builder")
            && !vm_module.contains("pub use script_builder::ScriptBuilder"),
        "neo_core::neo_vm should not expose ScriptBuilder once script construction has a \
         top-level module"
    );
}

#[test]
fn call_flags_is_owned_by_neo_primitives() {
    // CallFlags is a shared permission-flag type both the VM host and the
    // smart-contract layer reference. The A1/S1 seam moved it DOWN into
    // neo-primitives (Layer 0) so neither layer imports it upward — breaking the
    // old neo_vm <-> smart_contract cycle. smart_contract/call_flags.rs keeps a
    // thin re-export for the historical `neo_core::smart_contract::CallFlags`
    // path; the neo-vm host must not own or expose it.
    let workspace = workspace_root();
    let vm_module = read_source(workspace.join("neo-vm/src/lib.rs"));
    let primitives_call_flags =
        read_source(workspace.join("neo-primitives/src/call_flags.rs"));
    let smart_contract_call_flags =
        read_source(workspace.join("neo-core/src/smart_contract/call_flags.rs"));

    assert!(
        primitives_call_flags.contains("CallFlags"),
        "CallFlags should be defined in neo-primitives/src/call_flags.rs (Layer 0), shared \
         by the VM host and smart-contract layer without an upward dependency"
    );
    assert!(
        smart_contract_call_flags.contains("pub use neo_primitives::CallFlags"),
        "smart_contract::call_flags should re-export CallFlags from neo-primitives, not \
         redefine it"
    );
    assert!(
        !workspace.join("neo-vm/src/call_flags.rs").exists(),
        "CallFlags must not live in the neo-vm host crate"
    );
    assert!(
        !vm_module.contains("pub mod call_flags")
            && !vm_module.contains("pub use call_flags::CallFlags"),
        "the neo-vm host should not own or expose CallFlags; it comes from neo-primitives"
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
                .join(format!("neo-vm/src/{module}.rs"))
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
    let reference_counter =
        read_source(workspace.join("neo-vm/src/reference_counter.rs"));

    assert!(
        !workspace
            .join("neo-vm/src/i_reference_counter.rs")
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
            .join("neo-vm/src/strongly_connected_components")
            .exists()
            && !vm_module.contains("pub mod strongly_connected_components"),
        "neo-core should not keep a local strongly_connected_components facade"
    );
    assert!(
        !workspace
            .join("neo-vm/src/stack_item/stack_item_vertex.rs")
            .exists()
            && !stack_item_module.contains("pub mod stack_item_vertex")
            && !stack_item_module.contains("pub use stack_item_vertex"),
        "compound stack item id allocation should not keep a local stack_item_vertex facade"
    );
    assert!(
        reference_counter.contains("use neo_vm_rs::Tarjan;")
            && !reference_counter
                .contains("crate::neo_vm::strongly_connected_components::Tarjan::new()"),
        "ReferenceCounter should use the shared Tarjan implementation directly"
    );

    for relative in [
        "neo-vm/src/stack_item/array.rs",
        "neo-vm/src/stack_item/buffer.rs",
        "neo-vm/src/stack_item/map.rs",
        "neo-vm/src/stack_item/struct_item.rs",
    ] {
        let source = read_source(workspace.join(relative));
        assert!(
            source.contains("next_stack_item_id")
                && !source.contains("stack_item_vertex::next_stack_item_id"),
            "{relative} should allocate compound ids through neo-vm-rs directly"
        );
    }

    let mut shared_tarjan = neo_vm_rs::Tarjan::new();
    shared_tarjan.add_edge(1, 2);
    shared_tarjan.add_edge(2, 1);
    assert_eq!(shared_tarjan.find_components().len(), 1);
}

#[test]
fn local_stack_item_view_shims_are_removed() {
    let workspace = workspace_root();
    let stack_item_module = read_source(workspace.join("neo-vm/src/stack_item/mod.rs"));
    let compound_jump_table =
        read_source(workspace.join("neo-vm/src/jump_table/compound.rs"));

    for relative in [
        "neo-vm/src/stack_item/primitive_type.rs",
        "neo-vm/src/stack_item/compound_type.rs",
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
        "neo-vm/src/stack_item/boolean.rs",
        "neo-vm/src/stack_item/integer.rs",
        "neo-vm/src/stack_item/null.rs",
    ] {
        assert!(
            !workspace.join(relative).exists(),
            "{relative} should be removed; primitive values should use StackItem or \
             neo_vm_rs::StackValue directly"
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
            .join("neo-vm/src/stack_item/interop_interface.rs")
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
        workspace.join("neo-tokens-tracker/src/trackers/nep_11/nep11_balance_key.rs"),
    );
    let transfer_key = read_source(
        workspace.join("neo-tokens-tracker/src/trackers/nep_11/nep11_transfer_key.rs"),
    );
    let nep11_module =
        read_source(workspace.join("neo-tokens-tracker/src/trackers/nep_11/mod.rs"));

    assert!(
        !workspace
            .join("neo-vm/src/stack_item/byte_string.rs")
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
        !workspace.join("neo-vm/src/debugger.rs").exists(),
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
            .join("neo-vm/src/application_engine.rs")
            .exists(),
        "unused local neo_vm ApplicationEngine facade should be deleted; production \
         blockchain-aware execution is owned by smart_contract::ApplicationEngine"
    );
    assert!(
        !vm_module.contains("pub mod application_engine")
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
        !workspace.join("neo-vm/src/tests").exists(),
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
            source.contains("use neo_vm_rs::VmState;"),
            "{relative} should use neo_vm_rs::VmState directly for final RPC VM states"
        );
        assert!(
            !contains_neo_core_vmstate_import(&source),
            "{relative} should not import local neo_core::neo_vm::VMState for final RPC states"
        );
    }

    let helper =
        read_source(workspace.join("neo-rpc/src/client/models/vm_state_utils.rs"));
    assert!(
        helper.contains("pub fn vm_state_to_string(state: VmState)"),
        "RPC VM-state formatting should accept neo_vm_rs::VmState directly"
    );
    assert!(
        helper.contains("pub fn vm_state_from_str(value: &str) -> Option<VmState>"),
        "RPC VM-state parsing should return neo_vm_rs::VmState directly"
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
    let helper =
        read_source(workspace.join("neo-rpc/src/server/smart_contract/helpers.rs"));

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
    let source =
        read_source(workspace.join("neo-rpc/src/server/rpc_server_blockchain/tests.rs"));

    assert!(
        source.contains("use neo_vm_rs::{OpCode, StackValue};")
            && source.contains("BinarySerializer::serialize_stack_value"),
        "RPC blockchain tests should seed native-contract storage through neo_vm_rs::StackValue"
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
            source.contains("use neo_vm_rs::StackValue;"),
            "{relative} should use neo_vm_rs::StackValue directly for RPC stack payloads"
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
fn rpc_server_imports_host_runtime_through_vm_runtime_boundary() {
    let workspace = workspace_root();
    for relative in [
        "neo-rpc/src/server/diagnostic.rs",
        "neo-rpc/src/server/session.rs",
        "neo-rpc/src/server/rpc_server_tokens_tracker/mod.rs",
        "neo-rpc/src/server/smart_contract/helpers.rs",
        "neo-rpc/src/server/smart_contract/invocation.rs",
        "neo-rpc/src/server/smart_contract/tests.rs",
    ] {
        let source = fs::read_to_string(workspace.join(relative)).unwrap();
        assert!(
            source.contains("neo_core::vm_runtime"),
            "{relative} should import host-runtime-only local VM types through the \
             vm_runtime boundary"
        );
        assert!(
            !source.contains("neo_core::neo_vm"),
            "{relative} should not import host-runtime-only local VM types through the \
             neo_vm implementation tree"
        );
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
    let script_builder =
        read_source(workspace.join("neo-core/src/script_builder.rs"));
    assert!(
        script_builder.contains("pub fn emit_push_stack_value")
            && script_builder.contains("neo_vm_rs::StackValue"),
        "ScriptBuilder should accept neo_vm_rs::StackValue directly for callers that no \
         longer need local StackItem identity"
    );

    let helpers =
        read_source(workspace.join("neo-rpc/src/server/smart_contract/helpers.rs"));
    assert!(
        helpers.contains("contract_parameter_to_stack_value")
            && helpers.contains("emit_push_stack_value(item)"),
        "RPC invoke argument script construction should convert contract parameters to \
         neo_vm_rs::StackValue and push them directly"
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

fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("tests crate lives below workspace root")
        .to_path_buf()
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

fn contains_neo_core_opcode_import(source: &str) -> bool {
    let mut rest = source;
    while let Some(start) = rest.find("neo_core::neo_vm::{") {
        rest = &rest[start..];
        let import = rest.split_once(';').map_or(rest, |(import, _)| import);
        if import.contains("OpCode") {
            return true;
        }
        rest = &rest["neo_core::neo_vm::{".len()..];
    }
    false
}

fn contains_crate_neo_vm_opcode_import(source: &str) -> bool {
    let mut rest = source;
    while let Some(start) = rest.find("crate::neo_vm::{") {
        rest = &rest[start..];
        let import = rest.split_once(';').map_or(rest, |(import, _)| import);
        if import.contains("OpCode") {
            return true;
        }
        rest = &rest["crate::neo_vm::{".len()..];
    }
    false
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

fn contains_neo_vm_script_builder_import(source: &str) -> bool {
    source.contains("neo_core::neo_vm::ScriptBuilder")
        || source.contains("neo_core::neo_vm::script_builder::ScriptBuilder")
        || source.contains("crate::neo_vm::ScriptBuilder")
        || source.contains("crate::neo_vm::script_builder::ScriptBuilder")
        || contains_braced_neo_vm_import(source, "neo_core::neo_vm::{", "ScriptBuilder")
        || contains_braced_neo_vm_import(source, "crate::neo_vm::{", "ScriptBuilder")
}

fn contains_braced_neo_vm_import(source: &str, prefix: &str, symbol: &str) -> bool {
    let mut rest = source;
    while let Some(start) = rest.find(prefix) {
        rest = &rest[start..];
        let import = rest.split_once(';').map_or(rest, |(import, _)| import);
        if import.contains(symbol) {
            return true;
        }
        rest = &rest[prefix.len()..];
    }
    false
}

fn line_contains_opcode_repr_cast(line: &str) -> bool {
    if line.contains("instruction.opcode as u8") {
        return true;
    }

    let mut rest = line;
    while let Some(start) = rest.find("OpCode::") {
        let after_prefix = &rest[start + "OpCode::".len()..];
        let symbol_len = after_prefix
            .bytes()
            .take_while(|byte| byte.is_ascii_uppercase() || byte.is_ascii_digit() || *byte == b'_')
            .count();
        let after_symbol = &after_prefix[symbol_len..];
        if after_symbol.trim_start().starts_with("as u8") {
            return true;
        }
        rest = after_symbol;
    }

    false
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

fn execute_state(builder: ScriptBuilder) -> VMState {
    let mut engine = ExecutionEngine::new(None);
    engine
        .load_script(Script::new_relaxed(builder.to_array()), -1, 0)
        .unwrap();
    engine.execute()
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
