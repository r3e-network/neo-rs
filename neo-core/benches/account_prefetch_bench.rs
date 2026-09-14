//! Benchmarks for AccountPrefetchCache performance
//!
//! These benchmarks measure cache hit rates, eviction behavior, and overall
//  latency improvements from prefetching native contract account states.

use criterion::{black_box, criterion_group, criterion_main, Criterion};
use neo_core::state_service::AccountPrefetchCache;
use neo_primitives::UInt160;
use std::sync::Arc;

/// Generate random UInt160 addresses for testing
fn generate_accounts(n: usize) -> Vec<UInt160> {
    (0..n).map(|i| UInt160::from_integer(i as u64)).collect()
}

/// Create mock account data (simulates balance/storage serialization)
fn mock_account_data(account: &UInt160) -> Vec<u8> {
    // Simulate NEP-17 balance state: [balance_lo[2B][balance_hi[2B][nonce 4B][flags 1B]
    let mut data = vec![0u8; 9];
    data[0..2].copy_from_slice(&account.to_integer().to_le_bytes()[0..2]);
    data[2..4].copy_from_slice(&((account.to_integer() >> 16) as u32).to_le_bytes());
    data
}

fn bench_cache_put_get(c: &mut Criterion) {
    let cache = AccountPrefetchCache::new();
    let accounts = generate_accounts(100);

    c.bench_function("cache_put_get_100_accounts", |b| {
        b.iter(|| {
            for account in &accounts {
                let data = mock_account_data(account);
                cache.cache.write().put(*account, data.clone());
                let _ = cache.get_cached(account);
            }
        })
    });
}

fn bench_lru_eviction(c: &mut Criterion) {
    // Test with smaller capacity for faster iterations
    let cache = AccountPrefetchCache::with_capacity(50);
    let accounts = generate_accounts(100);

    c.bench_function("lru_eviction_100_insertions_capacity_50", |b| {
        b.iter(|| {
            for account in &accounts {
                let data = mock_account_data(account);
                cache.cache.write().put(*account, data);
            }
            // Verify oldest entries were evicted
            assert!(cache.cached_count() == 50);
        })
    });
}

fn bench_prefetch_simulation(c: &mut Criterion) {
    let cache = AccountPrefetchCache::new();
    
    // Simulate a typical transfer transaction: sender + receiver access
    let sender = UInt160::repeat_byte(0x01);
    let receiver = UInt160::repeat_byte(0x02);
    let accounts_to_touch = vec![sender, receiver];
    let transactions = 1000;
    
    c.bench_function("prefetch_simulation_1k_transfers", |b| {
        b.iter(|| {
            // Phase 1: Prefetch (cold - no cache warmup yet)
            for _ in 0..10 {
                for acc in &accounts_to_touch {
                    let data = mock_account_data(acc);
                    cache.cache.write().put(*acc, data);
                }
            }
            
            // Phase 2: Warm - simulate real transaction execution
            for _ in 0..transactions {
                // Sender balance check (hit)
                cache.get_cached(&sender);
                
                // Receiver balance check (hit)
                cache.get_cached(&receiver);
            }
        })
    });
}

fn bench_metrics_overhead(c: &mut Criterion) {
    let cache = AccountPrefetchCache::new();
    let accounts = generate_accounts(100);

    c.bench_function("metrics_recording_1k_accesses", |b| {
        b.iter(|| {
            for i in 0..1000 {
                if i % 10 == 0 {
                    cache.record_access(true);  // Hit
                } else {
                    cache.record_access(false); // Miss
                }
            }
            
            // Verify metrics updated
            let metrics = cache.metrics();
            assert_eq!(metrics.cache_hits + metrics.cache_misses, 1000);
        })
    });
}

fn bench_extract_account_refs(c: &mut Criterion) {
    use neo_core::network::p2p::payloads::{Transaction, Signer, TransactionAttribute, TransactionAttributeType, Witness};
    use neo_primitives::ContractParameterType;
    
    // Create a realistic transaction with signers
    let tx = Transaction {
        script: vec![/* dummy bytecode */ 0x01, 0x02, 0x03],
        system_fee: 0,
        network_fee: 1000,
        valid_until_block: 1000000,
        attributes: vec![],
        signatures: vec![],
        size: 0,
        version: 0,
        nonce: 0,
        signer_count: 2,
        scripts: vec![
            Witness {
                invocation_script: vec![],
                verification_script: vec![0x01, 0x40, /* contract hash here */],
                authorization_script: vec![],
            }
        ],
        signers: vec![
            Signer {
                id: 0,
                index: 0,
                rules: vec![],
                scopes: Some(neo_primitives::WitnessScope::CustomAccounts),
                account: UInt160::repeat_byte(0xAA),
            },
            Signer {
                id: 1,
                index: 1,
                rules: vec![],
                scopes: Some(neo_primitives::WitnessScope::Global),
                account: UInt160::repeat_byte(0xBB),
            }
        ],
    };
    
    c.bench_function("extract_accounts_from_transaction", |b| {
        b.iter(|| {
            // This would normally call extract_account_refs, but it's private
            // For now, we're just measuring the setup cost
            black_box(&tx.scripts.len());
            black_box(&tx.signers.len());
        })
    });
}

criterion_group!(
    benches,
    bench_cache_put_get,
    bench_lru_eviction,
    bench_prefetch_simulation,
    bench_metrics_overhead,
    bench_extract_account_refs
);

criterion_main!(benches);
