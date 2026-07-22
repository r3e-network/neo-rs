use super::*;

fn tx_with_signers_and_fees(nonce: u32, sys: i64, net: i64, accounts: &[UInt160]) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_system_fee(sys);
    tx.set_network_fee(net);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_signers(
        accounts
            .iter()
            .map(|a| Signer::new(*a, WitnessScope::NONE))
            .collect(),
    );
    tx.set_witnesses(accounts.iter().map(|_| Witness::empty()).collect());
    tx
}

/// C# `TransactionVerificationContext.CheckTransaction` rebates a conflict's
/// fees only when the v3.10.1 payer tuple matches. For ordinary transactions
/// the tuple is `(Signers[0], None)`. A conflict that merely lists the sender as
/// a later signer must NOT be rebated.
#[test]
fn conflict_rebate_keys_on_fee_payer_tuple_like_csharp() {
    let sender = UInt160::from_bytes(&[1u8; 20]).expect("sender");
    let other = UInt160::from_bytes(&[2u8; 20]).expect("other");

    // (a) first signer IS the sender -> rebated (7 + 3 = 10)
    let first_is_sender = PoolItem::new(
        tx_with_signers_and_fees(1, 7, 3, &[sender, other]),
        TransactionOrigin::Local,
    );
    // (b) first signer is someone else, sender appears later -> NOT rebated
    //     (the pre-fix bug rebated this because it matched ANY signer)
    let later_is_sender = PoolItem::new(
        tx_with_signers_and_fees(2, 100, 100, &[other, sender]),
        TransactionOrigin::Local,
    );
    // (c) sender absent entirely -> not rebated
    let unrelated = PoolItem::new(
        tx_with_signers_and_fees(3, 100, 100, &[other]),
        TransactionOrigin::Local,
    );

    let conflicts = vec![first_is_sender, later_is_sender, unrelated];
    assert_eq!(
        conflict_rebate(
            &conflicts,
            Some(FeePayer {
                primary: sender,
                secondary: None,
            })
        ),
        num_bigint::BigInt::from(10),
    );
    // No sender -> no rebate.
    assert_eq!(
        conflict_rebate(&conflicts, None),
        num_bigint::BigInt::from(0),
    );
}

#[test]
fn conflict_rebate_keys_notary_sponsored_conflicts_by_secondary_payer() {
    let notary = neo_native_contracts::Notary::script_hash();
    let payer = UInt160::from_bytes(&[3u8; 20]).expect("payer");
    let other = UInt160::from_bytes(&[4u8; 20]).expect("other");

    let sponsored = PoolItem::new(
        tx_with_signers_and_fees(4, 7, 3, &[notary, payer]),
        TransactionOrigin::Local,
    );
    let different_payer = PoolItem::new(
        tx_with_signers_and_fees(5, 100, 100, &[notary, other]),
        TransactionOrigin::Local,
    );

    let conflicts = vec![sponsored, different_payer];
    assert_eq!(
        conflict_rebate(
            &conflicts,
            Some(FeePayer {
                primary: notary,
                secondary: Some(payer),
            })
        ),
        num_bigint::BigInt::from(10),
    );
}

#[test]
fn verification_context_reserves_notary_sponsored_fees_by_secondary_payer() {
    let notary = neo_native_contracts::Notary::script_hash();
    let payer = UInt160::from_bytes(&[5u8; 20]).expect("payer");
    let other = UInt160::from_bytes(&[6u8; 20]).expect("other");
    let mut inner = MemoryPoolInner::with_capacity(8);

    inner.context_add(&tx_with_signers_and_fees(6, 7, 3, &[notary, payer]));
    inner.context_add(&tx_with_signers_and_fees(7, 100, 100, &[notary, other]));

    assert_eq!(
        inner.sender_fees.get(&FeePayer {
            primary: notary,
            secondary: Some(payer),
        }),
        Some(&num_bigint::BigInt::from(10)),
    );
    assert_eq!(
        inner.sender_fees.get(&FeePayer {
            primary: notary,
            secondary: Some(other),
        }),
        Some(&num_bigint::BigInt::from(200)),
    );
}

#[derive(Debug)]
struct FailingContainsTransactionProvider;

impl AdmissionLedgerProvider for FailingContainsTransactionProvider {
    fn current_index<B: neo_storage::CacheRead>(
        &self,
        _snapshot: &neo_storage::DataCache<B>,
    ) -> neo_error::CoreResult<u32> {
        Ok(0)
    }

    fn contains_transaction<B: neo_storage::CacheRead>(
        &self,
        _snapshot: &neo_storage::DataCache<B>,
        _hash: &UInt256,
    ) -> neo_error::CoreResult<bool> {
        Err(neo_error::CoreError::invalid_operation(
            "injected contains-transaction failure",
        ))
    }

    fn contains_conflict_hash<B: neo_storage::CacheRead>(
        &self,
        _snapshot: &neo_storage::DataCache<B>,
        _hash: &UInt256,
        _signers: &[UInt160],
        _max_traceable_blocks: u32,
    ) -> neo_error::CoreResult<bool> {
        Ok(false)
    }
}

#[test]
fn canonical_admission_fails_closed_on_ledger_provider_error() {
    let (settings, snapshot, private, public, account) = fixture(0x5A);
    let pool = memory_pool(&settings);
    let tx = signed_tx(&settings, &private, &public, account, 90, 1, Vec::new());
    let hash = tx.hash();

    match pool.add_transaction(
        TransactionOrigin::External,
        tx,
        &snapshot,
        &FailingContainsTransactionProvider,
    ) {
        TransactionAdmissionOutcome::Error {
            hash: Some(actual_hash),
            origin: TransactionOrigin::External,
            error: TransactionAdmissionError::ProviderRead { operation, .. },
        } => {
            assert_eq!(actual_hash, hash);
            assert_eq!(operation, "contains_transaction");
        }
        outcome => panic!("expected typed provider error, got {outcome:?}"),
    }
    assert!(!pool.contains(&hash));
}

#[test]
fn canonical_admission_retains_private_origin_across_block_invalidation() {
    let (settings, snapshot, private, public, account) = fixture(0x5B);
    let pool = memory_pool(&settings);
    let tx = signed_tx(&settings, &private, &public, account, 91, 1, Vec::new());
    let hash = tx.hash();
    let provider = NativeAdmissionLedgerProvider::new();

    let outcome = pool.add_transaction(TransactionOrigin::Private, tx, &snapshot, &provider);
    assert!(outcome.is_accepted(), "unexpected outcome: {outcome:?}");
    assert_eq!(outcome.origin(), TransactionOrigin::Private);
    assert_eq!(
        pool.get_verified(&hash)
            .expect("verified transaction")
            .origin,
        TransactionOrigin::Private
    );

    assert!(pool.update_pool_for_block_persisted(&[]).is_empty());
    assert_eq!(
        pool.get(&hash).expect("invalidated transaction").origin,
        TransactionOrigin::Private
    );
    assert_eq!(pool.verified_count(), 0);
    assert_eq!(pool.unverified_count(), 1);
}
