use super::*;
use crate::native_contract_provider::{
    NativeContractProvider, NoNativeContract, NoNativeContractProvider,
};
use neo_vm_rs::OpCode;
use std::sync::Arc;

struct CurrentIndexProvider(u32);

impl NativeContractProvider for CurrentIndexProvider {
    type Contract = NoNativeContract;

    fn current_block_index<B: neo_storage::CacheRead>(
        &self,
        _snapshot: &neo_storage::DataCache<B>,
    ) -> CoreResult<u32> {
        Ok(self.0)
    }
}

fn handler_id(table: &JumpTable, opcode: OpCode) -> usize {
    table
        .get(opcode)
        .expect("opcode handler should be registered") as usize
}

fn registered_services(engine: &ApplicationEngine) -> Vec<(String, i64, u8)> {
    let mut services: Vec<_> = engine
        .host_syscall_registrations
        .iter()
        .map(|(name, price, flags)| (name.clone(), *price, flags.bits()))
        .collect();
    services.sort();
    services
}

#[test]
fn negative_fee_faults_before_whitelist_like_csharp_v3101() {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = ApplicationEngine::<CurrentIndexProvider>::new_with_shared_block_and_native_contract_provider(
        TriggerType::Application,
        None,
        snapshot,
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        NoDiagnostic,
        Arc::new(CurrentIndexProvider(0)),
    )
    .expect("engine");

    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("load entry script");
    engine
        .current_execution_state()
        .expect("current execution state")
        .lock()
        .whitelisted = true;

    let error = engine
        .add_fee_datoshi(-1)
        .expect_err("negative AddFee must fault before whitelist bypass");

    assert!(error.to_string().contains("Negative gas fee"));
    assert_eq!(engine.fee_consumed_pico(), 0);
}

#[test]
fn gorgon_selects_default_jump_table_like_csharp_v3101() {
    // C# `ApplicationEngine.Create`: `HF_Gorgon` enabled -> `DefaultJumpTable`.
    let mut settings = ProtocolSettings::default();
    settings.hardforks.clear();
    settings.hardforks.insert(Hardfork::HfEchidna, 0);
    settings.hardforks.insert(Hardfork::HfGorgon, 0);
    let selected = ApplicationEngine::<NoNativeContractProvider>::select_jump_table(&settings, 0);
    let default = JumpTable::default();

    for opcode in [OpCode::SHL, OpCode::SHR, OpCode::HASKEY, OpCode::PICKITEM] {
        assert_eq!(
            handler_id(&selected, opcode),
            handler_id(&default, opcode),
            "{opcode:?} should use the default post-Gorgon handler"
        );
    }
}

#[test]
fn echidna_without_gorgon_selects_not_gorgon_table_like_csharp_v3101() {
    // C# `ApplicationEngine.Create`: `HF_Echidna` enabled but `HF_Gorgon` not ->
    // `NotGorgonJumpTable` = default with HASKEY/PICKITEM/SETITEM/REMOVE reverted
    // to their pre-neo-vm#543 handlers. SHL/SHR are unchanged from the default.
    let mut settings = ProtocolSettings::default();
    settings.hardforks.clear();
    settings.hardforks.insert(Hardfork::HfEchidna, 0);
    let selected = ApplicationEngine::<NoNativeContractProvider>::select_jump_table(&settings, 0);
    let default = JumpTable::default();
    let not_gorgon = JumpTable::not_gorgon();

    // The selected table is exactly the NotGorgon table.
    for opcode in [
        OpCode::SHL,
        OpCode::SHR,
        OpCode::HASKEY,
        OpCode::PICKITEM,
        OpCode::SETITEM,
        OpCode::REMOVE,
    ] {
        assert_eq!(
            handler_id(&selected, opcode),
            handler_id(&not_gorgon, opcode),
            "{opcode:?} should use the NotGorgon handler under Echidna"
        );
    }

    // SHL/SHR match the default (no Gorgon split); the compound opcodes differ.
    assert_eq!(
        handler_id(&selected, OpCode::SHL),
        handler_id(&default, OpCode::SHL)
    );
    assert_eq!(
        handler_id(&selected, OpCode::SHR),
        handler_id(&default, OpCode::SHR)
    );
    for opcode in [
        OpCode::HASKEY,
        OpCode::PICKITEM,
        OpCode::SETITEM,
        OpCode::REMOVE,
    ] {
        assert_ne!(
            handler_id(&selected, opcode),
            handler_id(&default, opcode),
            "{opcode:?} must revert to the pre-543 handler under Echidna-without-Gorgon"
        );
    }
}

#[test]
fn jump_table_selection_uses_supplied_native_provider_current_index() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.clear();
    settings.hardforks.insert(Hardfork::HfEchidna, 1);
    settings.hardforks.insert(Hardfork::HfGorgon, 10);
    let selected = ApplicationEngine::<NoNativeContractProvider>::select_jump_table(&settings, 10);
    let default = JumpTable::default();

    assert_eq!(
        handler_id(&selected, OpCode::HASKEY),
        handler_id(&default, OpCode::HASKEY),
        "constructor-time jump table selection must use the injected provider current index"
    );
}

#[test]
fn engine_native_provider_is_fixed_at_construction() {
    let provider = Arc::new(CurrentIndexProvider(7));
    let engine = ApplicationEngine::<CurrentIndexProvider>::new_with_shared_block_and_native_contract_provider(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        NoDiagnostic,
        Arc::clone(&provider),
    )
    .expect("engine with native provider");

    assert!(std::ptr::eq(
        engine.native_contract_provider(),
        provider.as_ref()
    ));
    assert_eq!(engine.current_block_index(), 7);
}

fn expected_base_services() -> Vec<(String, i64, u8)> {
    let mut services = vec![
        (
            "System.Contract.Call".to_string(),
            1 << 15,
            (CallFlags::READ_STATES | CallFlags::ALLOW_CALL).bits(),
        ),
        (
            "System.Contract.CallNative".to_string(),
            0,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Contract.CreateMultisigAccount".to_string(),
            0,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Contract.CreateStandardAccount".to_string(),
            0,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Contract.GetCallFlags".to_string(),
            1 << 10,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Contract.NativeOnPersist".to_string(),
            0,
            CallFlags::STATES.bits(),
        ),
        (
            "System.Contract.NativePostPersist".to_string(),
            0,
            CallFlags::STATES.bits(),
        ),
        (
            "System.Crypto.CheckMultisig".to_string(),
            0,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Crypto.CheckSig".to_string(),
            1 << 15,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Iterator.Next".to_string(),
            1 << 15,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Iterator.Value".to_string(),
            1 << 4,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Runtime.BurnGas".to_string(),
            1 << 4,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Runtime.CheckWitness".to_string(),
            1 << 10,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Runtime.CurrentSigners".to_string(),
            1 << 4,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Runtime.GasLeft".to_string(),
            1 << 4,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Runtime.GetAddressVersion".to_string(),
            1 << 3,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Runtime.GetCallingScriptHash".to_string(),
            1 << 4,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Runtime.GetEntryScriptHash".to_string(),
            1 << 4,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Runtime.GetExecutingScriptHash".to_string(),
            1 << 4,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Runtime.GetInvocationCounter".to_string(),
            1 << 4,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Runtime.GetNetwork".to_string(),
            1 << 3,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Runtime.GetNotifications".to_string(),
            1 << 12,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Runtime.GetRandom".to_string(),
            0,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Runtime.GetScriptContainer".to_string(),
            1 << 3,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Runtime.GetTime".to_string(),
            1 << 3,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Runtime.GetTrigger".to_string(),
            1 << 3,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Runtime.LoadScript".to_string(),
            1 << 15,
            CallFlags::ALLOW_CALL.bits(),
        ),
        (
            "System.Runtime.Log".to_string(),
            1 << 15,
            CallFlags::ALLOW_NOTIFY.bits(),
        ),
        (
            "System.Runtime.Notify".to_string(),
            1 << 15,
            CallFlags::ALLOW_NOTIFY.bits(),
        ),
        (
            "System.Runtime.Platform".to_string(),
            1 << 3,
            CallFlags::NONE.bits(),
        ),
        (
            "System.Storage.AsReadOnly".to_string(),
            1 << 4,
            CallFlags::READ_STATES.bits(),
        ),
        (
            "System.Storage.Delete".to_string(),
            1 << 15,
            CallFlags::WRITE_STATES.bits(),
        ),
        (
            "System.Storage.Find".to_string(),
            1 << 15,
            CallFlags::READ_STATES.bits(),
        ),
        (
            "System.Storage.Get".to_string(),
            1 << 15,
            CallFlags::READ_STATES.bits(),
        ),
        (
            "System.Storage.GetContext".to_string(),
            1 << 4,
            CallFlags::READ_STATES.bits(),
        ),
        (
            "System.Storage.GetReadOnlyContext".to_string(),
            1 << 4,
            CallFlags::READ_STATES.bits(),
        ),
        (
            "System.Storage.Put".to_string(),
            1 << 15,
            CallFlags::WRITE_STATES.bits(),
        ),
    ];
    services.sort();
    services
}

fn expected_faun_services() -> Vec<(String, i64, u8)> {
    let mut services = expected_base_services();
    services.extend([
        (
            "System.Storage.Local.Delete".to_string(),
            1 << 15,
            CallFlags::WRITE_STATES.bits(),
        ),
        (
            "System.Storage.Local.Find".to_string(),
            1 << 15,
            CallFlags::READ_STATES.bits(),
        ),
        (
            "System.Storage.Local.Get".to_string(),
            1 << 15,
            CallFlags::READ_STATES.bits(),
        ),
        (
            "System.Storage.Local.Put".to_string(),
            1 << 15,
            CallFlags::WRITE_STATES.bits(),
        ),
    ]);
    services.sort();
    services
}

fn engine_with_settings(settings: ProtocolSettings) -> ApplicationEngine {
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

fn engine_with_settings_at_block(
    settings: ProtocolSettings,
    block_index: u32,
) -> ApplicationEngine {
    let mut block = Block::new();
    block.header.set_index(block_index);
    ApplicationEngine::<NoNativeContractProvider>::new_with_native_contract_provider(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        Some(block),
        settings,
        TEST_MODE_GAS,
        NoDiagnostic,
        Arc::new(NoNativeContractProvider),
    )
    .expect("application engine")
}

#[test]
fn interop_registry_matches_csharp_v3101_before_faun() {
    let engine = engine_with_settings_at_block(ProtocolSettings::default(), 0);

    assert_eq!(registered_services(&engine), expected_base_services());
}

#[test]
fn interop_registry_matches_csharp_v3101_from_faun() {
    let mut settings = ProtocolSettings::default();
    for hardfork in Hardfork::all() {
        settings.hardforks.insert(hardfork, 0);
    }

    let engine = engine_with_settings(settings);

    assert_eq!(registered_services(&engine), expected_faun_services());
}

/// Consensus-parity: C# Neo (v3.10.1) has **no** instruction-count cap on the
/// execution path — bounding is done purely by gas. neo-rs previously enforced
/// a 1,000,000-instruction cap (from the upstream `ExecutionEngineLimits`
/// default) that would FAULT a long, cheap-instruction script that C# HALTs,
/// causing a state divergence during block persistence. This test drives a
/// tight loop that executes well over 1,000,000 cheap opcodes and asserts the
/// engine HALTs (does not FAULT) when given sufficient gas.
#[test]
fn long_cheap_loop_halts_without_instruction_cap_like_csharp_v3101() {
    // Loop body (3 instructions per iteration): DEC, DUP, JMPIF back to DEC.
    // With a counter of 400_000 that is 3 * 400_000 = 1_200_000 loop
    // instructions, plus the initial push and the trailing DROP/RET — safely
    // above the old 1_000_000 cap.
    const ITERATIONS: i64 = 400_000;

    // Byte layout:
    //   [0..]  PUSHINT32 <ITERATIONS>   (initial counter)
    //   loop:  DEC                      <- loop_start
    //          DUP
    //          JMPIF loop               (offset = loop_start - jmpif_pos = -2)
    //          DROP
    //          RET
    let mut script = Vec::new();
    script.push(OpCode::PUSHINT32.byte());
    script.extend_from_slice(&(ITERATIONS as i32).to_le_bytes());

    let loop_start = script.len() as i32;
    script.push(OpCode::DEC.byte());
    script.push(OpCode::DUP.byte());
    let jmpif_pos = script.len() as i32;
    script.push(OpCode::JMPIF.byte());
    // Short-form JMPIF operand is a single signed byte relative to the JMPIF
    // opcode position; jump back to loop_start (DEC).
    script.push(((loop_start - jmpif_pos) as i8) as u8);
    script.push(OpCode::DROP.byte());
    script.push(OpCode::RET.byte());

    // TEST_MODE_GAS (200 GAS) is far more than the ~1.2M cheap opcodes cost, so
    // gas is not the limiting factor — the only thing that could fault this is
    // an instruction-count cap, which must no longer exist.
    let mut engine = engine_with_settings(ProtocolSettings::default());
    engine
        .load_script(script, CallFlags::ALL, None)
        .expect("load loop script");

    let state = engine.execute_allow_fault();

    assert_eq!(
        state,
        VMState::HALT,
        "a >1,000,000-instruction cheap loop must HALT (gas is the only bound), \
         not FAULT on an instruction cap; fault: {:?}",
        engine.fault_exception()
    );
}

#[test]
fn application_engine_diagnostic_is_typed_not_trait_object() {
    let engine_source = include_str!("../../application_engine/mod.rs");
    let state_source = include_str!("../../application_engine/state.rs");

    assert!(
        engine_source.contains(
            "pub struct ApplicationEngine<P = NoNativeContractProvider, D = NoDiagnostic, B = EmptyCacheBacking>"
        ),
        "ApplicationEngine should expose provider, diagnostic, and cache backing as explicit generic parameters"
    );
    assert!(
        engine_source.contains("diagnostic: D"),
        "ApplicationEngine should store the concrete diagnostic sink, not an erased object"
    );
    assert!(
        !engine_source.contains("Box<dyn Diagnostic>"),
        "engine state must not reintroduce boxed diagnostic dispatch"
    );
    assert!(
        !state_source.contains("Box<dyn Diagnostic>"),
        "engine constructors must accept typed diagnostics, not boxed trait objects"
    );
}

#[test]
fn application_engine_native_provider_is_a_required_typed_dependency() {
    let engine_source = include_str!("../../application_engine/mod.rs");
    let state_source = include_str!("../../application_engine/state.rs");

    assert!(
        engine_source.contains("native_contract_provider: Arc<P>"),
        "ApplicationEngine should always own its concrete native provider"
    );
    assert!(
        !engine_source.contains("native_contract_provider: Option<Arc<P>>"),
        "provider availability belongs in the provider type, not runtime engine state"
    );
    assert!(
        !state_source.contains("native_contract_provider: Option<Arc<P>>"),
        "provider-aware constructors should require a concrete provider value"
    );
    assert!(
        state_source.contains("fn native_contract_provider(&self) -> &P"),
        "engine internals should access the mandatory provider without optional branching"
    );
}

#[test]
fn application_engine_constructors_do_not_publish_a_movable_self_pointer() {
    let state_source = include_str!("../../application_engine/state.rs");
    let attach_host_offset = state_source
        .find("pub(super) fn attach_host")
        .expect("attach_host definition");
    let constructors = &state_source[..attach_host_offset];

    assert!(
        !constructors.contains("app.attach_host()"),
        "constructors return the engine by value, so host binding must wait until a callback-capable operation"
    );
    assert!(
        state_source.contains("fn attach_host(&mut self) -> bool"),
        "nested callback operations must know whether they own the host binding"
    );
    assert!(
        state_source.contains("fn detach_host(&mut self, attached_here: bool)"),
        "only the operation that installed the host may clear it"
    );
}

#[test]
fn vm_host_binding_is_scoped_to_callback_capable_operations() {
    let mut engine = engine_with_settings(ProtocolSettings::default());
    assert!(engine.vm_engine.engine().interop_host_ptr().is_none());

    engine
        .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
        .expect("load script");
    assert!(
        engine.vm_engine.engine().interop_host_ptr().is_none(),
        "script loading must not retain a self-pointer after returning"
    );

    assert_eq!(engine.execute_allow_fault(), VMState::HALT);
    assert!(
        engine.vm_engine.engine().interop_host_ptr().is_none(),
        "execution must not retain a self-pointer after returning"
    );
}

#[test]
fn nested_vm_host_binding_is_released_by_its_outer_owner() {
    let mut engine = engine_with_settings(ProtocolSettings::default());

    let outer = engine.attach_host();
    let inner = engine.attach_host();
    assert!(outer);
    assert!(!inner);

    engine.detach_host(inner);
    assert!(engine.vm_engine.engine().interop_host_ptr().is_some());

    engine.detach_host(outer);
    assert!(engine.vm_engine.engine().interop_host_ptr().is_none());
}
