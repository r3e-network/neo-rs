use super::*;

#[tokio::test]
async fn canonical_admission_maps_the_mempool_verdict() {
    let rejected = transaction_with_nonce(201);
    let (rejecting, _handle, _snapshot) = fixture_with_mempool_result(VerifyResult::PolicyFail);
    let rejected_reply = rejecting
        .add_transaction(TransactionOrigin::External, rejected)
        .await;
    assert_eq!(rejected_reply.result, VerifyResult::PolicyFail);

    let accepted = transaction_with_nonce(202);
    let accepted_hash = accepted.try_hash().expect("accepted hash");
    let (accepting, _handle, _snapshot) = fixture_with_mempool_result(VerifyResult::Succeed);
    let accepted_reply = accepting
        .add_transaction(TransactionOrigin::Local, accepted)
        .await;
    assert_eq!(accepted_reply.result, VerifyResult::Succeed);
    assert_eq!(accepted_reply.hash, accepted_hash);
}

#[tokio::test]
async fn canonical_admission_reports_already_exists_for_persisted_ledger_tx() {
    let (service, _handle, snapshot) = real_mempool_store_fixture();
    let mut tx = transaction_with_nonce(203);
    tx.set_signers(vec![neo_payloads::Signer::new(
        neo_primitives::UInt160::from_bytes(&[0x22; 20]).expect("signer"),
        neo_primitives::WitnessScope::NONE,
    )]);
    tx.set_witnesses(vec![neo_payloads::Witness::empty()]);
    let tx_hash = tx.try_hash().expect("tx hash");
    let mut key = Vec::with_capacity(33);
    key.push(11);
    key.extend_from_slice(&tx_hash.to_bytes());
    let value = neo_native_contracts::LedgerContract::new()
        .serialize_persisted_transaction_state(7, neo_vm::VmState::HALT, &tx)
        .expect("transaction state");
    snapshot.add(
        neo_storage::StorageKey::new(neo_native_contracts::LedgerContract::ID, key),
        neo_storage::StorageItem::from_bytes(value),
    );

    let reply = service
        .add_transaction(TransactionOrigin::External, tx)
        .await;
    assert_eq!(reply.result, VerifyResult::AlreadyExists);
    assert_eq!(reply.hash, tx_hash);
}

#[tokio::test]
async fn canonical_admission_reports_traceable_ledger_conflict() {
    let (service, _handle, snapshot) = real_mempool_store_fixture();
    seed_current_ledger(snapshot.as_ref(), 0);
    let signer = neo_primitives::UInt160::from_bytes(&[0x44; 20]).expect("signer");
    let mut tx = transaction_with_nonce(204);
    tx.set_signers(vec![neo_payloads::Signer::new(
        signer,
        neo_primitives::WitnessScope::NONE,
    )]);
    tx.set_witnesses(vec![neo_payloads::Witness::empty()]);
    let tx_hash = tx.try_hash().expect("tx hash");
    seed_conflict_record(snapshot.as_ref(), &tx_hash, &signer, 0);

    let reply = service
        .add_transaction(TransactionOrigin::External, tx)
        .await;
    assert_eq!(reply.result, VerifyResult::HasConflicts);
    assert_eq!(reply.hash, tx_hash);
}
