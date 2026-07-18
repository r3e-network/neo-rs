use super::*;
use crate::native_contract_provider::{
    NativeContractProvider, NoNativeContract, NoNativeContractProvider,
};
use neo_storage::{StorageItem, StorageKey};
use neo_vm::OpCode;
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

struct SnapshotPolicyProvider;

impl SnapshotPolicyProvider {
    const EXEC_FEE_FACTOR_KEY: &'static [u8] = b"exec-fee-factor";
    const STORAGE_PRICE_KEY: &'static [u8] = b"storage-price";
    const STORAGE_ID: i32 = 91;

    fn key(suffix: &[u8]) -> StorageKey {
        StorageKey::new(Self::STORAGE_ID, suffix.to_vec())
    }

    fn read_u32<B: neo_storage::CacheRead>(
        snapshot: &DataCache<B>,
        suffix: &[u8],
    ) -> CoreResult<u32> {
        let value = snapshot
            .get(&Self::key(suffix))
            .ok_or_else(|| CoreError::invalid_operation("missing test Policy value"))?
            .to_value();
        let bytes: [u8; 4] = value
            .try_into()
            .map_err(|_| CoreError::invalid_operation("malformed test Policy value"))?;
        Ok(u32::from_le_bytes(bytes))
    }
}

impl NativeContractProvider for SnapshotPolicyProvider {
    type Contract = NoNativeContract;

    fn exec_fee_factor_raw<B: neo_storage::CacheRead>(
        &self,
        snapshot: &DataCache<B>,
    ) -> CoreResult<u32> {
        Self::read_u32(snapshot, Self::EXEC_FEE_FACTOR_KEY)
    }

    fn storage_price<B: neo_storage::CacheRead>(&self, snapshot: &DataCache<B>) -> CoreResult<u32> {
        Self::read_u32(snapshot, Self::STORAGE_PRICE_KEY)
    }
}

fn put_u32(cache: &DataCache, suffix: &[u8], value: u32) {
    cache.update(
        SnapshotPolicyProvider::key(suffix),
        StorageItem::from_bytes(value.to_le_bytes().to_vec()),
    );
}

#[derive(Debug, Default)]
struct DisabledPostDiagnostic {
    post_calls: u64,
}

impl Diagnostic for DisabledPostDiagnostic {
    fn enabled(&self) -> bool {
        false
    }

    fn initialized(&mut self) {}

    fn disposed(&mut self) {}

    fn context_loaded<B: neo_storage::CacheRead>(
        &mut self,
        _context: &crate::ApplicationExecutionContext<B>,
    ) {
    }

    fn context_unloaded<B: neo_storage::CacheRead>(
        &mut self,
        _context: &crate::ApplicationExecutionContext<B>,
    ) {
    }

    fn pre_execute_instruction(&mut self, _instruction: &Instruction) {}

    fn post_execute_instruction(&mut self, _instruction: &Instruction) {
        self.post_calls += 1;
    }
}

fn handler_id(table: &JumpTable, opcode: OpCode) -> usize {
    table
        .get(opcode)
        .expect("opcode handler should be registered") as usize
}

fn registered_services(engine: &ApplicationEngine) -> Vec<(String, i64, u8)> {
    let mut services: Vec<_> = engine
        .vm_engine
        .engine()
        .interop_service()
        .expect("application engine interop service")
        .registered_descriptors()
        .map(|(name, price, flags)| (name.to_string(), price, flags.bits()))
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
        Arc::new(CurrentIndexProvider(8_800_000)),
    )
    .expect("engine");

    assert!(engine.fee_whitelist_enabled);

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
fn disabled_diagnostic_skips_only_optional_post_instruction_host_callbacks() {
    let mut disabled = ApplicationEngine::<NoNativeContractProvider, DisabledPostDiagnostic>::new_with_shared_block_and_native_contract_provider(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::mainnet(),
        TEST_MODE_GAS,
        DisabledPostDiagnostic::default(),
        Arc::new(NoNativeContractProvider),
    )
    .expect("disabled diagnostic engine");
    disabled
        .load_script(
            vec![OpCode::NOP.byte(), OpCode::RET.byte()],
            CallFlags::ALL,
            None,
        )
        .expect("load disabled diagnostic script");
    assert_eq!(disabled.execute_allow_fault(), VMState::HALT);
    assert_eq!(disabled.instructions_executed(), 2);
    assert_eq!(disabled.diagnostic.post_calls, 0);

    let mut enabled = ApplicationEngine::<NoNativeContractProvider, crate::InstructionCounter>::new_with_shared_block_and_native_contract_provider(
        TriggerType::Application,
        None,
        Arc::new(DataCache::new(false)),
        None,
        ProtocolSettings::mainnet(),
        TEST_MODE_GAS,
        crate::InstructionCounter::new(),
        Arc::new(NoNativeContractProvider),
    )
    .expect("enabled diagnostic engine");
    enabled
        .load_script(
            vec![OpCode::NOP.byte(), OpCode::RET.byte()],
            CallFlags::ALL,
            None,
        )
        .expect("load enabled diagnostic script");
    assert_eq!(enabled.execute_allow_fault(), VMState::HALT);
    assert_eq!(enabled.diagnostic.executed_count, 2);
}

#[test]
fn fee_whitelist_lookup_starts_at_mainnet_faun_height() {
    for (height, whitelist_enabled, expected_fee) in
        [(8_799_999, false, FEE_FACTOR), (8_800_000, true, 0)]
    {
        let mut engine = ApplicationEngine::<CurrentIndexProvider>::new_with_shared_block_and_native_contract_provider(
            TriggerType::Application,
            None,
            Arc::new(DataCache::new(false)),
            None,
            ProtocolSettings::mainnet(),
            TEST_MODE_GAS,
            NoDiagnostic,
            Arc::new(CurrentIndexProvider(height)),
        )
        .expect("engine");

        assert_eq!(engine.fee_whitelist_enabled, whitelist_enabled);
        engine
            .load_script(vec![OpCode::RET.byte()], CallFlags::ALL, None)
            .expect("load entry script");
        engine
            .current_execution_state()
            .expect("current execution state")
            .lock()
            .whitelisted = true;

        engine
            .add_fee_datoshi(1)
            .expect("positive fee should be accepted");
        assert_eq!(
            engine.fee_consumed_pico(),
            expected_fee,
            "fee whitelist behavior at height {height}"
        );
    }
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
    // to their pre-neo-vm#543 handlers and SHL/SHR reverted to pre-neo-vm#567.
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

    // All six pre-Gorgon handlers differ from the default.
    for opcode in [
        OpCode::HASKEY,
        OpCode::PICKITEM,
        OpCode::SETITEM,
        OpCode::REMOVE,
        OpCode::SHL,
        OpCode::SHR,
    ] {
        assert_ne!(
            handler_id(&selected, opcode),
            handler_id(&default, opcode),
            "{opcode:?} must revert to the pre-543 handler under Echidna-without-Gorgon"
        );
    }
}

#[test]
fn before_echidna_selects_not_echidna_composed_from_not_gorgon() {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.clear();
    settings.hardforks.insert(Hardfork::HfEchidna, 10);
    settings.hardforks.insert(Hardfork::HfGorgon, 20);
    let selected = ApplicationEngine::<NoNativeContractProvider>::select_jump_table(&settings, 9);
    let not_echidna = JumpTable::not_echidna();
    let default = JumpTable::default();

    for opcode in [
        OpCode::SUBSTR,
        OpCode::SHL,
        OpCode::SHR,
        OpCode::HASKEY,
        OpCode::PICKITEM,
        OpCode::SETITEM,
        OpCode::REMOVE,
    ] {
        assert_eq!(
            handler_id(&selected, opcode),
            handler_id(&not_echidna, opcode),
            "{opcode:?} should use the NotEchidna handler before Echidna"
        );
        assert_ne!(
            handler_id(&selected, opcode),
            handler_id(&default, opcode),
            "{opcode:?} should not use the post-fork default before Echidna"
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

#[test]
fn prepare_next_transaction_refreshes_policy_values_without_resetting_snapshot_changes() {
    // Neo v3.10.1 Blockchain.Persist creates each transaction engine from the
    // latest cloned snapshot; ApplicationEngine then reads both Policy prices.
    const OLD_EXEC_FEE_FACTOR: u32 = 30_0000;
    const OLD_STORAGE_PRICE: u32 = 100_000;
    const NEW_EXEC_FEE_FACTOR: u32 = 42_0000;
    const NEW_STORAGE_PRICE: u32 = 123_456;

    let base = DataCache::new(false);
    put_u32(
        &base,
        SnapshotPolicyProvider::EXEC_FEE_FACTOR_KEY,
        OLD_EXEC_FEE_FACTOR,
    );
    put_u32(
        &base,
        SnapshotPolicyProvider::STORAGE_PRICE_KEY,
        OLD_STORAGE_PRICE,
    );
    base.commit();

    let first_transaction_snapshot = Arc::new(base.clone_cache());
    let next_transaction_snapshot = Arc::new(base.clone_cache());
    put_u32(
        next_transaction_snapshot.as_ref(),
        SnapshotPolicyProvider::EXEC_FEE_FACTOR_KEY,
        NEW_EXEC_FEE_FACTOR,
    );
    put_u32(
        next_transaction_snapshot.as_ref(),
        SnapshotPolicyProvider::STORAGE_PRICE_KEY,
        NEW_STORAGE_PRICE,
    );
    let pending_key = StorageKey::new(92, b"pending-transaction-write".to_vec());
    next_transaction_snapshot.add(
        pending_key.clone(),
        StorageItem::from_bytes(vec![0xAA, 0x55]),
    );
    let pending_changes = next_transaction_snapshot.pending_change_count();
    assert_eq!(pending_changes, 3);

    let mut block = Block::new();
    block.header.set_index(8_800_000);
    let mut engine = ApplicationEngine::<SnapshotPolicyProvider>::new_with_shared_block_and_native_contract_provider(
        TriggerType::Application,
        None,
        first_transaction_snapshot,
        Some(Arc::new(block)),
        ProtocolSettings::mainnet(),
        TEST_MODE_GAS,
        NoDiagnostic,
        Arc::new(SnapshotPolicyProvider),
    )
    .expect("engine with snapshot-backed Policy provider");

    assert_eq!(engine.exec_fee_factor_raw(), OLD_EXEC_FEE_FACTOR);
    assert_eq!(engine.storage_price(), OLD_STORAGE_PRICE);

    engine.prepare_next_transaction(None, Arc::clone(&next_transaction_snapshot), TEST_MODE_GAS);

    assert_eq!(engine.exec_fee_factor_raw(), NEW_EXEC_FEE_FACTOR);
    assert_eq!(engine.storage_price(), NEW_STORAGE_PRICE);
    assert_eq!(
        next_transaction_snapshot.pending_change_count(),
        pending_changes,
        "refreshing cached Policy values must not reset transaction writes"
    );
    assert_eq!(
        next_transaction_snapshot
            .get(&pending_key)
            .expect("pending transaction write")
            .to_value(),
        vec![0xAA, 0x55]
    );

    engine.prepare_next_transaction(None, Arc::new(DataCache::new(false)), TEST_MODE_GAS);
    assert_eq!(engine.exec_fee_factor_raw(), 30 * FEE_FACTOR as u32);
    assert_eq!(engine.storage_price(), 100_000);
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
fn ordinary_engine_has_no_live_observation_allocation() {
    let engine = engine_with_settings(ProtocolSettings::default());
    assert!(!engine.execution_observations_enabled());
    assert!(engine.execution_observation_handle().is_none());
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
