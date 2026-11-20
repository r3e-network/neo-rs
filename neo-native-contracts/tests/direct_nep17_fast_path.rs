//! Differential tests for direct native NEP-17 dispatch versus the canonical VM.

use neo_native_contracts::{GasToken, NativeContract};
use neo_primitives::{CallFlags, TriggerType, UInt160, Verifiable, WitnessScope};
use neo_protocol::ProtocolSettings;
use neo_protocol::execution::{Block, BlockHeader};
use neo_protocol::payloads::Transaction;
use neo_protocol::signer::Signer;
use neo_smart_contract::application_engine::{
    ApplicationEngine, EngineNativeServices, NativeStateReader, Nep17Parser, TEST_MODE_GAS,
    TransferParams,
};
use neo_smart_contract::{ContractState, CoreError, CoreResult};
use neo_storage::persistence::DataCache;
use neo_vm::{OpCode, ScriptBuilder, VmState};
use num_bigint::BigInt;
use std::sync::Arc;

fn block() -> Block {
    Block::new(
        BlockHeader::new(
            0,
            Default::default(),
            Default::default(),
            1_468_595_301_000,
            0,
            1,
            0,
            UInt160::zero(),
            vec![],
        ),
        vec![],
    )
}

fn transfer_script(contract: UInt160, from: UInt160, to: UInt160, amount: i64) -> Vec<u8> {
    let mut builder = ScriptBuilder::new();
    builder.emit_opcode(OpCode::PUSHNULL);
    builder.emit_push_int(amount);
    builder.emit_push(&to.to_bytes());
    builder.emit_push(&from.to_bytes());
    builder.emit_push_int(4);
    builder.emit_opcode(OpCode::PACK);
    builder.emit_push_int(CallFlags::ALL.bits() as i64);
    builder.emit_push(b"transfer");
    builder.emit_push(&contract.to_bytes());
    builder.emit_syscall("System.Contract.Call").unwrap();
    builder.emit_opcode(OpCode::ASSERT);
    builder.to_array()
}

fn transaction(script: &[u8], witness: UInt160) -> Arc<dyn Verifiable> {
    let mut transaction = Transaction::new();
    transaction.set_script(script.to_vec());
    transaction.set_signers(vec![Signer::new(witness, WitnessScope::CALLED_BY_ENTRY)]);
    Arc::new(transaction)
}

fn make_engine_with_services(
    container: Option<Arc<dyn Verifiable>>,
    snapshot: Arc<DataCache>,
    services: EngineNativeServices,
) -> ApplicationEngine {
    ApplicationEngine::new_with_shared_block(
        TriggerType::Application,
        container,
        snapshot,
        Some(Arc::new(
            neo_protocol::execution::conversions::convert_ledger_block(block()),
        )),
        ProtocolSettings::default(),
        TEST_MODE_GAS,
        None,
        services,
    )
    .expect("application engine")
}

fn make_engine(
    container: Option<Arc<dyn Verifiable>>,
    snapshot: Arc<DataCache>,
) -> ApplicationEngine {
    make_engine_with_services(
        container,
        snapshot,
        neo_native_contracts::composition::default_native_services(),
    )
}

fn funded_snapshot(gas: &GasToken, account: UInt160, amount: i64) -> Arc<DataCache> {
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = make_engine(None, Arc::clone(&snapshot));
    gas.mint(&mut engine, &account, &BigInt::from(amount), false)
        .expect("fund test account");
    snapshot
}

#[derive(Debug)]
struct ExecutionObservation {
    state: VmState,
    fee: i64,
    exception: Option<String>,
    result_stack: String,
    notifications: String,
    logs: String,
    storage: Vec<(Vec<u8>, String, String)>,
}

fn observe(engine: &ApplicationEngine) -> ExecutionObservation {
    let mut storage: Vec<_> = engine
        .original_snapshot_cache()
        .tracked_items()
        .into_iter()
        .map(|(key, entry)| {
            (
                key.to_array(),
                format!("{:?}", entry.state),
                hex::encode(entry.item.to_value()),
            )
        })
        .collect();
    storage.sort_by(|left, right| left.0.cmp(&right.0));
    ExecutionObservation {
        state: engine.state(),
        fee: engine.fee_consumed(),
        exception: engine.fault_exception().map(str::to_owned),
        result_stack: format!("{:?}", engine.result_stack().to_vec()),
        notifications: format!("{:?}", engine.notifications()),
        logs: format!("{:?}", engine.logs()),
        storage,
    }
}

fn assert_direct_matches_canonical(
    params: &TransferParams,
    script: &[u8],
    container: Arc<dyn Verifiable>,
    snapshot: Arc<DataCache>,
) -> (ExecutionObservation, ExecutionObservation) {
    let canonical_snapshot = Arc::new(snapshot.fork_isolated());
    let direct_snapshot = Arc::new(snapshot.fork_isolated());
    let mut canonical = make_engine(
        Some(Arc::clone(&container)),
        Arc::clone(&canonical_snapshot),
    );
    canonical
        .load_script(script.to_vec(), CallFlags::ALL, None)
        .unwrap();
    let canonical_state = canonical.execute_allow_fault();
    assert_eq!(canonical_state, canonical.state());

    let mut direct = make_engine(Some(container), Arc::clone(&direct_snapshot));
    direct
        .load_script(script.to_vec(), CallFlags::ALL, None)
        .unwrap();
    assert!(direct.can_execute_direct_nep17_transfer(params));
    let direct_state = direct.execute_direct_nep17_transfer(params);
    assert_eq!(direct_state, direct.state());

    let canonical_observation = observe(&canonical);
    let direct_observation = observe(&direct);
    assert_eq!(canonical_observation.state, direct_observation.state);
    assert_eq!(canonical_observation.fee, direct_observation.fee);
    assert_eq!(
        canonical.instructions_executed(),
        direct.instructions_executed(),
        "direct dispatch must charge the instruction count canonical execution charges"
    );
    assert_eq!(
        canonical_observation.exception,
        direct_observation.exception
    );
    assert_eq!(
        canonical_observation.result_stack,
        direct_observation.result_stack
    );
    assert_eq!(
        canonical_observation.notifications,
        direct_observation.notifications
    );
    assert_eq!(canonical_observation.logs, direct_observation.logs);
    assert_eq!(canonical_observation.storage, direct_observation.storage);
    (canonical_observation, direct_observation)
}

#[test]
fn successful_gas_transfer_has_exact_receipt_and_storage_parity() {
    let gas = GasToken::new();
    let from = UInt160::from_bytes(&[1; 20]).unwrap();
    let to = UInt160::from_bytes(&[2; 20]).unwrap();
    let script = transfer_script(gas.hash(), from, to, 1000);
    let params = Nep17Parser::parse_transfer_script(&script).unwrap();
    let snapshot = funded_snapshot(&gas, from, 10_000);
    let (canonical, _) = assert_direct_matches_canonical(
        &params,
        &script,
        transaction(&script, from),
        Arc::clone(&snapshot),
    );
    assert_eq!(canonical.state, VmState::HALT);
    assert!(canonical.notifications.contains("Transfer"));
    assert_eq!(
        gas.balance_of_snapshot(snapshot.as_ref(), &from),
        BigInt::from(10_000)
    );
}

#[test]
fn zero_and_self_transfers_match_canonical() {
    let gas = GasToken::new();
    let from = UInt160::from_bytes(&[3; 20]).unwrap();
    let to = UInt160::from_bytes(&[4; 20]).unwrap();
    for (recipient, amount) in [(to, 0), (from, 25)] {
        let script = transfer_script(gas.hash(), from, recipient, amount);
        let params = Nep17Parser::parse_transfer_script(&script).unwrap();
        let snapshot = funded_snapshot(&gas, from, 1000);
        let (canonical, _) =
            assert_direct_matches_canonical(&params, &script, transaction(&script, from), snapshot);
        assert_eq!(canonical.state, VmState::HALT);
    }
}

#[test]
fn negative_insufficient_and_wrong_witness_match_fault_or_assert_semantics() {
    let gas = GasToken::new();
    let from = UInt160::from_bytes(&[5; 20]).unwrap();
    let to = UInt160::from_bytes(&[6; 20]).unwrap();
    let stranger = UInt160::from_bytes(&[7; 20]).unwrap();
    for (amount, witness, expected) in [
        (-1, from, VmState::FAULT),
        (1001, from, VmState::FAULT),
        (10, stranger, VmState::FAULT),
    ] {
        let script = transfer_script(gas.hash(), from, to, amount);
        let params = Nep17Parser::parse_transfer_script(&script).unwrap();
        let snapshot = funded_snapshot(&gas, from, 1000);
        let (canonical, direct) = assert_direct_matches_canonical(
            &params,
            &script,
            transaction(&script, witness),
            snapshot,
        );
        assert_eq!(canonical.state, expected);
        assert_eq!(direct.state, expected);
        assert!(canonical.exception.is_some());
    }
}

#[test]
fn non_native_and_contract_recipient_decline_direct_dispatch() {
    let gas = GasToken::new();
    let from = UInt160::from_bytes(&[8; 20]).unwrap();
    let non_native = UInt160::from_bytes(&[9; 20]).unwrap();
    let script = transfer_script(non_native, from, from, 1);
    let params = Nep17Parser::parse_transfer_script(&script).unwrap();
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = make_engine(None, snapshot);
    engine.load_script(script, CallFlags::ALL, None).unwrap();
    assert!(!engine.can_execute_direct_nep17_transfer(&params));

    let script = transfer_script(gas.hash(), from, gas.hash(), 0);
    let params = Nep17Parser::parse_transfer_script(&script).unwrap();
    let snapshot = Arc::new(DataCache::new(false));
    let mut engine = make_engine(None, snapshot);
    engine.load_script(script, CallFlags::ALL, None).unwrap();
    assert!(!engine.can_execute_direct_nep17_transfer(&params));
}

/// Answers one contract hash with a scripted outcome and forwards every other
/// read to the standard snapshot-backed reader.
struct ScriptedContractLookup {
    target: UInt160,
    outcome: ScriptedLookupOutcome,
    inner: Arc<dyn NativeStateReader>,
}

enum ScriptedLookupOutcome {
    /// The hash holds a deployed contract.
    DeployedContract,
    /// The snapshot read fails.
    ReadError,
}

impl NativeStateReader for ScriptedContractLookup {
    fn contract(&self, snapshot: &DataCache, hash: &UInt160) -> CoreResult<Option<ContractState>> {
        if *hash != self.target {
            return self.inner.contract(snapshot, hash);
        }
        match self.outcome {
            ScriptedLookupOutcome::DeployedContract => Ok(Some(ContractState::new_native(
                1,
                *hash,
                "ScriptedContract".to_string(),
            ))),
            ScriptedLookupOutcome::ReadError => Err(CoreError::invalid_operation(
                "scripted contract lookup failure",
            )),
        }
    }

    fn current_index(&self, snapshot: &DataCache) -> CoreResult<u32> {
        self.inner.current_index(snapshot)
    }

    fn is_blocked(&self, snapshot: &DataCache, hash: &UInt160) -> CoreResult<bool> {
        self.inner.is_blocked(snapshot, hash)
    }

    fn whitelisted_fee(
        &self,
        snapshot: &DataCache,
        hash: &UInt160,
        method: &str,
        param_count: u32,
    ) -> CoreResult<Option<i64>> {
        self.inner
            .whitelisted_fee(snapshot, hash, method, param_count)
    }

    fn oracle_original_signers(
        &self,
        snapshot: &DataCache,
        id: u64,
    ) -> CoreResult<Option<Vec<Signer>>> {
        self.inner.oracle_original_signers(snapshot, id)
    }
}

fn engine_with_scripted_lookup(
    script: &[u8],
    witness: UInt160,
    target: UInt160,
    outcome: ScriptedLookupOutcome,
) -> ApplicationEngine {
    let mut services = neo_native_contracts::composition::default_native_services();
    let inner = Arc::clone(&services.state_reader);
    services.state_reader = Arc::new(ScriptedContractLookup {
        target,
        outcome,
        inner,
    });
    let mut engine = make_engine_with_services(
        Some(transaction(script, witness)),
        Arc::new(DataCache::new(false)),
        services,
    );
    engine
        .load_script(script.to_vec(), CallFlags::ALL, None)
        .unwrap();
    engine
}

fn engine_with_instruction_limit(
    script: &[u8],
    witness: UInt160,
    snapshot: Arc<DataCache>,
    max_instructions: u64,
) -> ApplicationEngine {
    let mut engine = make_engine(Some(transaction(script, witness)), snapshot);
    let mut limits = *engine.execution_limits();
    limits.max_instructions = max_instructions;
    engine.set_execution_limits(limits);
    engine
        .load_script(script.to_vec(), CallFlags::ALL, None)
        .unwrap();
    engine
}

#[test]
fn contract_sender_declines_direct_dispatch() {
    let gas = GasToken::new();
    let sender = UInt160::from_bytes(&[10; 20]).unwrap();
    let recipient = UInt160::from_bytes(&[11; 20]).unwrap();
    let uninvolved = UInt160::from_bytes(&[12; 20]).unwrap();
    let script = transfer_script(gas.hash(), sender, recipient, 1);
    let params = Nep17Parser::parse_transfer_script(&script).unwrap();

    // A contract sender can be re-entered by the GAS reward callbacks the native
    // method triggers, so the transfer stays on the canonical VM.
    let mut contract_sender = engine_with_scripted_lookup(
        &script,
        sender,
        sender,
        ScriptedLookupOutcome::DeployedContract,
    );
    assert!(!contract_sender.can_execute_direct_nep17_transfer(&params));

    // The identical script stays eligible when the deployed contract is
    // uninvolved, so the refusal above is the sender check.
    let mut uninvolved_contract = engine_with_scripted_lookup(
        &script,
        sender,
        uninvolved,
        ScriptedLookupOutcome::DeployedContract,
    );
    assert!(uninvolved_contract.can_execute_direct_nep17_transfer(&params));
}

#[test]
fn recipient_lookup_error_declines_direct_dispatch() {
    let gas = GasToken::new();
    let from = UInt160::from_bytes(&[13; 20]).unwrap();
    let to = UInt160::from_bytes(&[14; 20]).unwrap();
    let uninvolved = UInt160::from_bytes(&[15; 20]).unwrap();
    let script = transfer_script(gas.hash(), from, to, 10);
    let params = Nep17Parser::parse_transfer_script(&script).unwrap();

    // "Cannot prove it is an account" is not "it is an account": a failed
    // recipient read declines the acceleration.
    let mut failing_recipient =
        engine_with_scripted_lookup(&script, from, to, ScriptedLookupOutcome::ReadError);
    assert!(!failing_recipient.can_execute_direct_nep17_transfer(&params));

    // The same reader failure for an uninvolved hash keeps the transfer
    // eligible, so the refusal above is the recipient read.
    let mut failing_elsewhere =
        engine_with_scripted_lookup(&script, from, uninvolved, ScriptedLookupOutcome::ReadError);
    assert!(failing_elsewhere.can_execute_direct_nep17_transfer(&params));
}

#[test]
fn instruction_limit_is_respected_in_direct_dispatch() {
    let gas = GasToken::new();
    let from = UInt160::from_bytes(&[16; 20]).unwrap();
    let to = UInt160::from_bytes(&[17; 20]).unwrap();
    let script = transfer_script(gas.hash(), from, to, 100);
    let params = Nep17Parser::parse_transfer_script(&script).unwrap();
    // Entry instructions plus the native PUSH0; CallNative; RET method entry.
    let required = params.entry_instructions.len() as u64 + 3;

    let snapshot = funded_snapshot(&gas, from, 10_000);
    let mut exact = engine_with_instruction_limit(&script, from, Arc::clone(&snapshot), required);
    assert!(exact.can_execute_direct_nep17_transfer(&params));
    assert_eq!(exact.execute_direct_nep17_transfer(&params), VmState::HALT);
    assert_eq!(exact.instructions_executed(), required);
    assert_eq!(
        gas.balance_of_snapshot(snapshot.as_ref(), &to),
        BigInt::from(100)
    );

    let snapshot = funded_snapshot(&gas, from, 10_000);
    let mut starved = engine_with_instruction_limit(&script, from, snapshot, required - 1);
    assert!(!starved.can_execute_direct_nep17_transfer(&params));
    // The canonical interpreter owns the shortfall and faults as it always does.
    assert_eq!(starved.execute_allow_fault(), VmState::FAULT);
    assert!(
        starved
            .fault_exception()
            .is_some_and(|message| message.contains("Instruction limit exceeded"))
    );
}
