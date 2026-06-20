use super::*;
use neo_vm_rs::OpCode;

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
fn echidna_selects_default_jump_table_like_csharp_v3100() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.clear();
    settings.hardforks.insert(Hardfork::HfEchidna, 0);
    let snapshot = DataCache::new(false);

    let selected = ApplicationEngine::select_jump_table(&settings, None, &snapshot);
    let default = JumpTable::default();

    for opcode in [OpCode::SHL, OpCode::SHR, OpCode::HASKEY, OpCode::PICKITEM] {
        assert_eq!(
            handler_id(&selected, opcode),
            handler_id(&default, opcode),
            "{opcode:?} should use the default post-Echidna handler"
        );
    }
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
    ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        settings,
        TEST_MODE_GAS,
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
    ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        Some(block),
        settings,
        TEST_MODE_GAS,
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
