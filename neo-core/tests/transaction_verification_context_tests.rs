use neo_core::ledger::TransactionVerificationContext;
use neo_core::network::p2p::payloads::{
    oracle_response::OracleResponse, oracle_response_code::OracleResponseCode, signer::Signer,
    transaction::Transaction, transaction_attribute::TransactionAttribute,
};
use neo_core::persistence::DataCache;
use neo_core::UInt160;
use neo_core::WitnessScope;
use num_bigint::BigInt;

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
