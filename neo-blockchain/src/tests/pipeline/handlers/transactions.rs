use super::*;

#[tokio::test]
async fn fill_memory_pool_admits_transactions_through_mempool() {
    let (service, _handle) = fixture();
    let tx1 = transaction_with_nonce(101);
    let tx2 = transaction_with_nonce(102);
    let hash1 = tx1.try_hash().expect("tx1 hash");
    let hash2 = tx2.try_hash().expect("tx2 hash");

    service
        .handle_fill_memory_pool(FillMemoryPool {
            transactions: vec![tx1, tx2],
        })
        .await;

    assert!(service.ledger.get_transaction(&hash1).is_some());
    assert!(service.ledger.get_transaction(&hash2).is_some());
}

#[tokio::test]
async fn preverify_completed_uses_mempool_verdict_before_caching() {
    let (rejecting, _handle) = fixture_with_mempool_result(VerifyResult::PolicyFail);
    let rejected = transaction_with_nonce(201);
    let rejected_hash = rejected.try_hash().expect("rejected hash");
    rejecting
        .handle_preverify_completed(crate::PreverifyCompleted {
            transaction: rejected,
            relay: true,
            result: VerifyResult::Succeed,
            cached_state_independent: Some(VerifyResult::Succeed),
        })
        .await;
    assert!(
        rejecting.ledger.get_transaction(&rejected_hash).is_none(),
        "state-dependent mempool rejection must not populate the ledger tx cache"
    );

    let (accepting, _handle) = fixture_with_mempool_result(VerifyResult::Succeed);
    let accepted = transaction_with_nonce(202);
    let accepted_hash = accepted.try_hash().expect("accepted hash");
    accepting
        .handle_preverify_completed(crate::PreverifyCompleted {
            transaction: accepted,
            relay: true,
            result: VerifyResult::Succeed,
            cached_state_independent: Some(VerifyResult::Succeed),
        })
        .await;
    assert!(accepting.ledger.get_transaction(&accepted_hash).is_some());
}

#[tokio::test]
async fn on_new_transaction_reports_already_exists_for_persisted_ledger_tx() {
    let (service, _handle, snapshot) = store_fixture();
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
        .serialize_persisted_transaction_state(7, neo_vm_rs::VmState::HALT, &tx)
        .expect("transaction state");
    snapshot.add(
        neo_storage::StorageKey::new(neo_native_contracts::LedgerContract::ID, key),
        neo_storage::StorageItem::from_bytes(value),
    );
    assert!(
        neo_native_contracts::LedgerContract::new()
            .contains_transaction(snapshot.as_ref(), &tx_hash)
            .expect("ledger contains transaction check"),
        "test fixture must seed a full Ledger transaction record"
    );

    assert_eq!(
        service.on_new_transaction(&tx, None),
        VerifyResult::AlreadyExists
    );
}

#[tokio::test]
async fn on_new_transaction_reports_has_conflicts_for_traceable_ledger_conflict() {
    let (service, _handle, snapshot) = store_fixture();
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

    assert_eq!(
        service.on_new_transaction(&tx, None),
        VerifyResult::HasConflicts
    );
    assert!(
        service.ledger.get_transaction(&tx_hash).is_none(),
        "C# Blockchain.OnNewTransaction rejects traceable ledger conflicts before mempool admission"
    );
}
