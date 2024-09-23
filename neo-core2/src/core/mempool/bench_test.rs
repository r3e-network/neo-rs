use std::collections::HashMap;
use test::Bencher;

use crate::core::transaction::{Transaction, Signer};
use crate::util::Uint160;

const POOL_SIZE: usize = 10000;

struct FeerStub {
    fee_per_byte: u64,
    block_height: u64,
    balance: u64,
}

#[bench]
fn benchmark_pool(b: &mut Bencher) {
    let fe = FeerStub {
        fee_per_byte: 1,
        block_height: 1,
        balance: 100_0000_0000,
    };

    let mut txes_simple = Vec::with_capacity(POOL_SIZE);
    for _ in 0..POOL_SIZE {
        let mut tx = Transaction::new(vec![1, 2, 3], 100500);
        tx.signers = vec![Signer { account: Uint160([1, 2, 3]) }];
        txes_simple.push(tx);
    }

    let mut txes_inc_fee = Vec::with_capacity(POOL_SIZE);
    for i in 0..POOL_SIZE {
        let mut tx = Transaction::new(vec![1, 2, 3], 100500);
        tx.network_fee = 10 * i as i64;
        tx.signers = vec![Signer { account: Uint160([1, 2, 3]) }];
        txes_inc_fee.push(tx);
    }

    let mut txes_multi = Vec::with_capacity(POOL_SIZE);
    for i in 0..POOL_SIZE {
        let mut tx = Transaction::new(vec![1, 2, 3], 100500);
        tx.signers = vec![Signer { account: Uint160([1, 2, 3, (i % 256) as u8, (i / 256) as u8]) }];
        txes_multi.push(tx);
    }

    let mut txes_multi_inc = Vec::with_capacity(POOL_SIZE);
    for i in 0..POOL_SIZE {
        let mut tx = Transaction::new(vec![1, 2, 3], 100500);
        tx.network_fee = 10 * i as i64;
        tx.signers = vec![Signer { account: Uint160([1, 2, 3, (i % 256) as u8, (i / 256) as u8]) }];
        txes_multi_inc.push(tx);
    }

    let mut senders: HashMap<&str, Vec<Transaction>> = HashMap::new();
    senders.insert("one, same fee", txes_simple);
    senders.insert("one, incr fee", txes_inc_fee);
    senders.insert("many, same fee", txes_multi);
    senders.insert("many, incr fee", txes_multi_inc);

    for (name, txes) in senders {
        b.bench_function(name, |b| {
            let mut p = Pool::new(POOL_SIZE, 0, false, None);
            b.iter(|| {
                for tx in &txes {
                    if p.add(tx, &fe).is_err() {
                        b.fail();
                    }
                }
                p.remove_stale(|_| false, &fe);
            });
        });
    }
}
