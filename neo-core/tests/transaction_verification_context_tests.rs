use neo_core::UInt160;
use neo_core::WitnessScope;
use neo_core::ledger::TransactionVerificationContext;
use neo_core::network::p2p::payloads::{
    notary_assisted::NotaryAssisted, oracle_response::OracleResponse,
    oracle_response_code::OracleResponseCode, signer::Signer, transaction::Transaction,
    transaction_attribute::TransactionAttribute,
};
use neo_core::persistence::DataCache;
use neo_core::smart_contract::native::notary::Deposit;
use neo_core::smart_contract::native::{NativeContract, Notary};
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
