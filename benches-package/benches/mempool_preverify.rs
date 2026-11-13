//! Deterministic serial versus parallel mempool preverification benchmarks.

use criterion::{BatchSize, Criterion, black_box, criterion_group, criterion_main};
use neo_primitives::{VerifyResult, WitnessScope};
use neo_protocol::get_sign_data_vec;
use neo_protocol::payloads::{Signer, Transaction};
use neo_protocol::protocol_settings::ProtocolSettings;
use neo_protocol::witness::Witness;
use neo_system::mempool::TransactionRouter;
use neo_vm::OpCode;
use neo_wallets::KeyPair;

fn signed_transaction(settings: &ProtocolSettings, nonce: u32) -> Transaction {
    let mut private_key = [0x42u8; 32];
    private_key[0] = nonce as u8;
    private_key[1] = (nonce >> 8) as u8;
    let keypair = KeyPair::from_private_key(&private_key).expect("benchmark keypair");
    let mut transaction = Transaction::new();
    transaction.set_nonce(nonce);
    transaction.set_valid_until_block(1_000_000);
    transaction.set_script(vec![OpCode::PUSH1.byte()]);
    transaction.set_signers(vec![Signer::new(
        keypair.get_script_hash(),
        WitnessScope::CALLED_BY_ENTRY,
    )]);
    let sign_data = get_sign_data_vec(&transaction, settings.network).expect("signing data");
    let signature = keypair.sign(&sign_data).expect("benchmark signature");
    let mut invocation = Vec::with_capacity(signature.len() + 2);
    invocation.push(OpCode::PUSHDATA1.byte());
    invocation.push(signature.len() as u8);
    invocation.extend_from_slice(&signature);
    transaction.set_witnesses(vec![Witness::new_with_scripts(
        invocation,
        keypair.get_verification_script(),
    )]);
    transaction
}

fn assert_all_succeed(results: &[neo_system::mempool::PreverifyCompleted]) {
    assert_eq!(results.len(), 64);
    assert!(
        results
            .iter()
            .all(|completed| completed.result == VerifyResult::Succeed)
    );
}

fn bench_preverify_batch(c: &mut Criterion) {
    let settings = ProtocolSettings::default();
    let router = TransactionRouter::new(settings.clone());
    let transactions = (0..64)
        .map(|nonce| (signed_transaction(&settings, nonce), true))
        .collect::<Vec<_>>();
    let mut group = c.benchmark_group("mempool_preverify_64_signed_transactions");

    group.bench_function("serial", |b| {
        b.iter_batched(
            || transactions.clone(),
            |batch| {
                let results = batch
                    .into_iter()
                    .map(|(transaction, relay)| router.preverify(transaction, relay))
                    .collect::<Vec<_>>();
                assert_all_succeed(&results);
                black_box(results);
            },
            BatchSize::SmallInput,
        );
    });

    group.bench_function("parallel_ordered", |b| {
        b.iter_batched(
            || transactions.clone(),
            |batch| {
                let results = router.preverify_batch(batch);
                assert_all_succeed(&results);
                black_box(results);
            },
            BatchSize::SmallInput,
        );
    });

    group.finish();
}

criterion_group!(benches, bench_preverify_batch);
criterion_main!(benches);
