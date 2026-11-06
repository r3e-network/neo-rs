use super::transaction::{
    verify_transaction_state_dependent, verify_transaction_state_dependent_at_height,
    verify_transaction_state_independent,
};
use super::transaction_context::TransactionVerificationContext;
use neo_native_contracts::notary::Deposit;
use neo_native_contracts::{LedgerContract, Notary};
use neo_primitives::{UInt160, UInt256, VerifyResult, WitnessScope};
use neo_protocol::payloads::oracle_response_code::OracleResponseCode;
use neo_protocol::payloads::{
    Block, NotaryAssisted, OracleResponse, Signer, Transaction, TransactionAttribute,
};
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_protocol::witness::Witness;
use neo_smart_contract::application_engine::native_contract::NativeContract;
use neo_storage::persistence::DataCache;
use neo_vm::OpCode;
use num_bigint::BigInt;
use num_traits::Zero;

fn build_transaction(network_fee: i64, system_fee: i64) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_network_fee(network_fee);
    tx.set_system_fee(system_fee);
    tx.set_script(vec![0x01]);
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::GLOBAL)]);
    tx
}

fn build_oracle_attribute(id: u64) -> TransactionAttribute {
    TransactionAttribute::OracleResponse(OracleResponse {
        id,
        code: OracleResponseCode::ConsensusUnreachable,
        result: Vec::new(),
    })
}

fn build_sponsored_transaction(
    notary: UInt160,
    payer: UInt160,
    network_fee: i64,
    system_fee: i64,
) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_network_fee(network_fee);
    tx.set_system_fee(system_fee);
    tx.set_script(vec![0x01]);
    tx.set_signers(vec![
        Signer::new(notary, WitnessScope::NONE),
        Signer::new(payer, WitnessScope::GLOBAL),
    ]);
    tx.set_attributes(vec![TransactionAttribute::NotaryAssisted(
        NotaryAssisted::new(1),
    )]);
    tx
}

fn build_inclusion_block() -> Block {
    let mut block = Block::new();
    block.header.set_index(100);
    let witness =
        Witness::new_with_scripts(Vec::new(), vec![OpCode::PUSHT.byte(), OpCode::RET.byte()]);
    let mut tx = Transaction::new();
    tx.set_nonce(100);
    tx.set_system_fee(0);
    tx.set_network_fee(100_000_000);
    tx.set_valid_until_block(block.index());
    tx.set_script(vec![OpCode::PUSHT.byte(), OpCode::RET.byte()]);
    tx.set_signers(vec![Signer::new(witness.script_hash(), WitnessScope::NONE)]);
    tx.set_witnesses(vec![witness]);
    block.transactions.push(tx);
    block
}

#[test]
fn inclusion_predecessor_height_accepts_transaction_expired_at_snapshot_height() {
    let snapshot = DataCache::new(false);
    let ledger = LedgerContract::new();
    let snapshot_hash = UInt256::from([0x81; 32]);
    ledger
        .set_current_block_state(&snapshot, &snapshot_hash, 100)
        .expect("persist snapshot height");
    let snapshot_height = ledger
        .current_index(&snapshot)
        .expect("read snapshot height");
    assert_eq!(snapshot_height, 100);
    assert_eq!(
        ledger.current_hash(&snapshot).expect("read snapshot hash"),
        snapshot_hash
    );
    let block = build_inclusion_block();
    let previous_height = block.index() - 1;
    let tx = &block.transactions[0];
    let mut settings = ProtocolSettings::default_settings();
    settings.max_valid_until_block_increment = 10;
    assert_eq!(previous_height, 99);
    assert_ne!(previous_height, snapshot_height);
    assert_eq!(tx.valid_until_block(), block.index());
    assert_eq!(
        verify_transaction_state_independent(tx, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        verify_transaction_state_dependent_at_height(
            tx,
            &settings,
            &snapshot,
            previous_height,
            None,
            &[]
        ),
        VerifyResult::Succeed
    );
    assert_eq!(
        verify_transaction_state_dependent_at_height(
            tx,
            &settings,
            &snapshot,
            block.index(),
            None,
            &[]
        ),
        VerifyResult::Expired
    );
    assert_eq!(
        verify_transaction_state_dependent(tx, &settings, &snapshot, None, &[]),
        VerifyResult::Expired
    );
    assert_eq!(
        ledger
            .current_index(&snapshot)
            .expect("unchanged snapshot height"),
        100
    );
    ledger
        .set_current_block_state(&snapshot, &snapshot_hash, previous_height)
        .expect("update snapshot height");
    assert_eq!(
        ledger
            .current_index(&snapshot)
            .expect("updated snapshot height"),
        99
    );
    assert_eq!(
        verify_transaction_state_dependent(tx, &settings, &snapshot, None, &[]),
        VerifyResult::Succeed
    );
}

#[test]
fn inclusion_predecessor_height_accepts_transaction_beyond_snapshot_validity_window() {
    let snapshot = DataCache::new(false);
    let ledger = LedgerContract::new();
    let snapshot_hash = UInt256::from([0x82; 32]);
    ledger
        .set_current_block_state(&snapshot, &snapshot_hash, 10)
        .expect("persist snapshot height");
    let snapshot_height = ledger
        .current_index(&snapshot)
        .expect("read snapshot height");
    assert_eq!(snapshot_height, 10);
    assert_eq!(
        ledger.current_hash(&snapshot).expect("read snapshot hash"),
        snapshot_hash
    );
    let block = build_inclusion_block();
    let previous_height = block.index() - 1;
    let tx = &block.transactions[0];
    let mut settings = ProtocolSettings::default_settings();
    settings.max_valid_until_block_increment = 10;
    assert_eq!(previous_height, 99);
    assert_ne!(previous_height, snapshot_height);
    assert_eq!(tx.valid_until_block(), 100);
    assert_eq!(
        verify_transaction_state_independent(tx, &settings),
        VerifyResult::Succeed
    );
    assert_eq!(
        verify_transaction_state_dependent_at_height(
            tx,
            &settings,
            &snapshot,
            previous_height,
            None,
            &[]
        ),
        VerifyResult::Succeed
    );
    assert_eq!(
        verify_transaction_state_dependent_at_height(
            tx,
            &settings,
            &snapshot,
            snapshot_height,
            None,
            &[]
        ),
        VerifyResult::NotYetValid
    );
    assert_eq!(
        verify_transaction_state_dependent(tx, &settings, &snapshot, None, &[]),
        VerifyResult::NotYetValid
    );
    assert_eq!(
        ledger
            .current_index(&snapshot)
            .expect("unchanged snapshot height"),
        10
    );
    ledger
        .set_current_block_state(&snapshot, &snapshot_hash, previous_height)
        .expect("update snapshot height");
    assert_eq!(
        ledger
            .current_index(&snapshot)
            .expect("updated snapshot height"),
        99
    );
    assert_eq!(
        verify_transaction_state_dependent(tx, &settings, &snapshot, None, &[]),
        VerifyResult::Succeed
    );
}

#[test]
fn duplicate_oracle_responses_are_rejected() {
    let snapshot = DataCache::new(true);
    let mut context =
        TransactionVerificationContext::with_balance_provider(|_, _| BigInt::from(10));
    let mut first = build_transaction(1, 2);
    first.set_attributes(vec![build_oracle_attribute(1)]);
    assert!(context.check_transaction(&first, std::iter::empty::<&Transaction>(), &snapshot));
    context.add_transaction(&first);
    let mut second = build_transaction(2, 1);
    second.set_attributes(vec![build_oracle_attribute(1)]);
    assert!(!context.check_transaction(&second, std::iter::empty::<&Transaction>(), &snapshot));
}

#[test]
fn sender_fee_accumulates_until_balance_exceeded() {
    let snapshot = DataCache::new(true);
    let mut context = TransactionVerificationContext::with_balance_provider(|_, _| BigInt::from(8));
    let tx = build_transaction(1, 2); // total fee = 3
    assert!(context.check_transaction(&tx, std::iter::empty::<&Transaction>(), &snapshot));
    context.add_transaction(&tx);
    assert!(context.check_transaction(&tx, std::iter::empty::<&Transaction>(), &snapshot));
    context.add_transaction(&tx);
    assert!(!context.check_transaction(&tx, std::iter::empty::<&Transaction>(), &snapshot));
    context.remove_transaction(&tx);
    assert!(context.check_transaction(&tx, std::iter::empty::<&Transaction>(), &snapshot));
    context.add_transaction(&tx);
    assert!(!context.check_transaction(&tx, std::iter::empty::<&Transaction>(), &snapshot));
}

#[test]
fn conflicts_release_fee_budget() {
    let snapshot = DataCache::new(true);
    let mut context = TransactionVerificationContext::with_balance_provider(|_, _| BigInt::from(7));
    let tx = build_transaction(1, 2); // fee = 3
    let conflict = build_transaction(1, 1); // fee = 2
    assert!(context.check_transaction(&tx, std::iter::empty::<&Transaction>(), &snapshot));
    context.add_transaction(&tx);
    assert!(context.check_transaction(&tx, std::iter::empty::<&Transaction>(), &snapshot));
    context.add_transaction(&tx);
    assert!(!context.check_transaction(&tx, std::iter::empty::<&Transaction>(), &snapshot));
    let conflicts = [conflict];
    let conflict_refs: Vec<&Transaction> = conflicts.iter().collect();
    assert!(context.check_transaction(&tx, conflict_refs, &snapshot));
}

#[test]
fn sponsored_transactions_charge_notary_deposit_by_secondary_payer() {
    let snapshot = DataCache::new(false);
    let notary = Notary::new();
    let payer = UInt160::from([7u8; UInt160::LENGTH]);
    notary.set_deposit(&snapshot, &payer, &Deposit::new(BigInt::from(8), 100));
    let mut context = TransactionVerificationContext::with_balance_provider(|_, _| BigInt::zero());
    let tx = build_sponsored_transaction(notary.hash(), payer, 3, 0);
    assert!(context.check_transaction(&tx, std::iter::empty::<&Transaction>(), &snapshot));
    context.add_transaction(&tx);
    assert!(context.check_transaction(&tx, std::iter::empty::<&Transaction>(), &snapshot));
    context.add_transaction(&tx);
    assert!(!context.check_transaction(&tx, std::iter::empty::<&Transaction>(), &snapshot));
    context.remove_transaction(&tx);
    assert!(context.check_transaction(&tx, std::iter::empty::<&Transaction>(), &snapshot));
}

#[test]
fn sponsored_conflicts_release_secondary_payer_budget() {
    let snapshot = DataCache::new(false);
    let notary = Notary::new();
    let payer = UInt160::from([6u8; UInt160::LENGTH]);
    notary.set_deposit(&snapshot, &payer, &Deposit::new(BigInt::from(5), 100));
    let mut context = TransactionVerificationContext::with_balance_provider(|_, _| BigInt::zero());
    let existing = build_sponsored_transaction(notary.hash(), payer, 4, 0);
    let replacement = build_sponsored_transaction(notary.hash(), payer, 4, 0);
    context.add_transaction(&existing);
    assert!(!context.check_transaction(
        &replacement,
        std::iter::empty::<&Transaction>(),
        &snapshot
    ));
    assert!(context.check_transaction(&replacement, [&existing], &snapshot));
}

#[test]
fn sponsored_fee_budgets_are_isolated_by_secondary_payer() {
    let snapshot = DataCache::new(false);
    let notary = Notary::new();
    let payer_a = UInt160::from([8u8; UInt160::LENGTH]);
    let payer_b = UInt160::from([9u8; UInt160::LENGTH]);
    notary.set_deposit(&snapshot, &payer_a, &Deposit::new(BigInt::from(3), 100));
    notary.set_deposit(&snapshot, &payer_b, &Deposit::new(BigInt::from(3), 100));
    let mut context = TransactionVerificationContext::with_balance_provider(|_, _| BigInt::zero());
    let tx_a = build_sponsored_transaction(notary.hash(), payer_a, 3, 0);
    let tx_b = build_sponsored_transaction(notary.hash(), payer_b, 3, 0);
    assert!(context.check_transaction(&tx_a, std::iter::empty::<&Transaction>(), &snapshot));
    context.add_transaction(&tx_a);
    assert!(context.check_transaction(&tx_b, std::iter::empty::<&Transaction>(), &snapshot));
}
