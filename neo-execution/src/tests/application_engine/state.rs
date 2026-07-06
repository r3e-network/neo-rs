use super::*;
use crate::native_contract_provider::{NativeContractLookup, NativeContractProvider};
use neo_vm_rs::OpCode;
use std::sync::Arc;

struct CurrentIndexProvider(u32);

impl NativeContractProvider for CurrentIndexProvider {
    fn get_native_contract(&self, _hash: &UInt160) -> Option<Arc<dyn NativeContract>> {
        None
    }

    fn get_native_contract_by_name(&self, _name: &str) -> Option<Arc<dyn NativeContract>> {
        None
    }

    fn all_native_contracts(&self) -> Vec<Arc<dyn NativeContract>> {
        Vec::new()
    }

    fn all_native_contract_hashes(&self) -> Vec<UInt160> {
        Vec::new()
    }

    fn current_block_index(&self, _snapshot: &DataCache) -> CoreResult<u32> {
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
fn gorgon_selects_default_jump_table_like_csharp_v3100() {
    // C# `ApplicationEngine.Create`: `HF_Gorgon` enabled -> `DefaultJumpTable`.
    let mut settings = ProtocolSettings::default();
    settings.hardforks.clear();
    settings.hardforks.insert(Hardfork::HfEchidna, 0);
    settings.hardforks.insert(Hardfork::HfGorgon, 0);
    let snapshot = DataCache::new(false);

    let selected = ApplicationEngine::select_jump_table(&settings, None, &snapshot, None);
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
fn echidna_without_gorgon_selects_not_gorgon_table_like_csharp_v3100() {
    // C# `ApplicationEngine.Create`: `HF_Echidna` enabled but `HF_Gorgon` not ->
    // `NotGorgonJumpTable` = default with HASKEY/PICKITEM/SETITEM/REMOVE reverted
    // to their pre-neo-vm#543 handlers. SHL/SHR are unchanged from the default.
    let mut settings = ProtocolSettings::default();
    settings.hardforks.clear();
    settings.hardforks.insert(Hardfork::HfEchidna, 0);
    let snapshot = DataCache::new(false);

    let selected = ApplicationEngine::select_jump_table(&settings, None, &snapshot, None);
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
    let snapshot = DataCache::new(false);
    let provider = Arc::new(CurrentIndexProvider(10)) as Arc<dyn NativeContractProvider>;

    let selected = ApplicationEngine::select_jump_table(&settings, None, &snapshot, Some(provider));
    let default = JumpTable::default();

    assert_eq!(
        handler_id(&selected, OpCode::HASKEY),
        handler_id(&default, OpCode::HASKEY),
        "constructor-time jump table selection must use the injected provider current index"
    );
}

#[test]
fn engine_native_provider_is_fixed_at_construction() {
    let engine = ApplicationEngine::new_with_shared_block_and_native_contract_provider(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
        None,
    )
    .expect("engine without native provider");

    let late_provider = Arc::new(CurrentIndexProvider(42)) as Arc<dyn NativeContractProvider>;
    NativeContractLookup::with_scoped_provider(late_provider, || {
        assert!(
            engine.native_contract_provider().is_none(),
            "an engine constructed without a provider must not observe later ambient provider changes"
        );
    });
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
    ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        settings,
        TEST_MODE_GAS,
        None,
        None,
    )
    .expect("application engine")
}

fn engine_with_settings_at_block(
    settings: ProtocolSettings,
    block_index: u32,
) -> ApplicationEngine {
    let mut block = Block::new();
    block.header.set_index(block_index);
    ApplicationEngine::new_with_native_contract_provider(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        Some(block),
        settings,
        TEST_MODE_GAS,
        None,
        None,
    )
    .expect("application engine")
}

#[test]
fn interop_registry_matches_csharp_v3100_before_faun() {
    let engine = engine_with_settings_at_block(ProtocolSettings::default(), 0);

    assert_eq!(registered_services(&engine), expected_base_services());
}

#[test]
fn interop_registry_matches_csharp_v3100_from_faun() {
    let mut settings = ProtocolSettings::default();
    for hardfork in Hardfork::all() {
        settings.hardforks.insert(hardfork, 0);
    }

    let engine = engine_with_settings(settings);

    assert_eq!(registered_services(&engine), expected_faun_services());
}

/// Consensus-parity: C# Neo (v3.10.0) has **no** instruction-count cap on the
/// execution path — bounding is done purely by gas. neo-rs previously enforced
/// a 1,000,000-instruction cap (from the upstream `ExecutionEngineLimits`
/// default) that would FAULT a long, cheap-instruction script that C# HALTs,
/// causing a state divergence during block persistence. This test drives a
/// tight loop that executes well over 1,000,000 cheap opcodes and asserts the
/// engine HALTs (does not FAULT) when given sufficient gas.
#[test]
fn long_cheap_loop_halts_without_instruction_cap_like_csharp_v3100() {
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
