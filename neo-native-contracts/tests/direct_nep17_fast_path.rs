//! Differential tests for direct native NEP-17 dispatch versus the canonical VM.

use neo_native_contracts::{GasToken, NativeContract};
use neo_primitives::{CallFlags, TriggerType, UInt160, Verifiable, WitnessScope};
use neo_protocol::ProtocolSettings;
use neo_protocol::execution::{Block, BlockHeader};
use neo_protocol::payloads::Transaction;
use neo_protocol::signer::Signer;
use neo_smart_contract::application_engine::{ApplicationEngine, TEST_MODE_GAS};
use neo_storage::persistence::DataCache;
use neo_vm::fast_path::{Nep17Parser, TransferParams};
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

fn make_engine(
    container: Option<Arc<dyn Verifiable>>,
    snapshot: Arc<DataCache>,
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
        neo_native_contracts::composition::default_native_services(),
    )
    .expect("application engine")
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
