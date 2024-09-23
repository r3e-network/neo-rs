use std::sync::{Arc, Mutex};
use std::sync::mpsc::{channel, Receiver};
use std::time::Duration;
use std::thread;

use crate::core::mempoolevent::{self, Event};
use crate::core::transaction::{self, Transaction, Signer};
use crate::util;
use crate::vm::opcode;
use crate::mempool::{Mempool, FeerStub};
use assert_matches::assert_matches;

#[test]
fn test_subscriptions() {
    let mut mp = Mempool::new(5, 0, false, None);
    assert!(std::panic::catch_unwind(|| mp.run_subscriptions()).is_err());
    assert!(std::panic::catch_unwind(|| mp.stop_subscriptions()).is_err());

    let fs = FeerStub { balance: 100 };
    let mut mp = Mempool::new(2, 0, true, None);
    mp.run_subscriptions();
    let (sub_tx1, sub_rx1): (std::sync::mpsc::Sender<Event>, Receiver<Event>) = channel();
    let (sub_tx2, sub_rx2): (std::sync::mpsc::Sender<Event>, Receiver<Event>) = channel();
    mp.subscribe_for_transactions(sub_tx1.clone());
    let mp_arc = Arc::new(Mutex::new(mp));
    let mp_clone = Arc::clone(&mp_arc);
    let handle = thread::spawn(move || {
        let mut mp = mp_clone.lock().unwrap();
        mp.stop_subscriptions();
    });

    let mut txs: Vec<Transaction> = Vec::with_capacity(4);
    for i in 0..4 {
        let mut tx = Transaction::new(vec![opcode::PUSH1 as u8], 0);
        tx.nonce = i as u32;
        tx.signers = vec![Signer { account: util::Uint160::new([1, 2, 3]) }];
        tx.network_fee = i as i64;
        txs.push(tx);
    }

    // add tx
    assert!(mp_arc.lock().unwrap().add(&txs[0], &fs).is_ok());
    assert!(wait_for_event(&sub_rx1, Duration::from_secs(1), Duration::from_millis(100), |event| {
        matches!(event, Event::TransactionAdded { tx } if tx == &txs[0])
    }));

    // several subscribers
    mp_arc.lock().unwrap().subscribe_for_transactions(sub_tx2.clone());
    assert!(mp_arc.lock().unwrap().add(&txs[1], &fs).is_ok());
    assert!(wait_for_event(&sub_rx1, Duration::from_secs(1), Duration::from_millis(100), |event| {
        matches!(event, Event::TransactionAdded { tx } if tx == &txs[1])
    }));
    assert!(wait_for_event(&sub_rx2, Duration::from_secs(1), Duration::from_millis(100), |event| {
        matches!(event, Event::TransactionAdded { tx } if tx == &txs[1])
    }));

    // reach capacity
    assert!(mp_arc.lock().unwrap().add(&txs[2], &FeerStub {}).is_ok());
    assert!(wait_for_event(&sub_rx1, Duration::from_secs(1), Duration::from_millis(100), |event| {
        matches!(event, Event::TransactionRemoved { tx } if tx == &txs[0])
    }));
    assert!(wait_for_event(&sub_rx2, Duration::from_secs(1), Duration::from_millis(100), |event| {
        matches!(event, Event::TransactionRemoved { tx } if tx == &txs[0])
    }));
    assert!(wait_for_event(&sub_rx1, Duration::from_secs(1), Duration::from_millis(100), |event| {
        matches!(event, Event::TransactionAdded { tx } if tx == &txs[2])
    }));
    assert!(wait_for_event(&sub_rx2, Duration::from_secs(1), Duration::from_millis(100), |event| {
        matches!(event, Event::TransactionAdded { tx } if tx == &txs[2])
    }));

    // remove tx
    mp_arc.lock().unwrap().remove(&txs[1].hash());
    assert!(wait_for_event(&sub_rx1, Duration::from_secs(1), Duration::from_millis(100), |event| {
        matches!(event, Event::TransactionRemoved { tx } if tx == &txs[1])
    }));
    assert!(wait_for_event(&sub_rx2, Duration::from_secs(1), Duration::from_millis(100), |event| {
        matches!(event, Event::TransactionRemoved { tx } if tx == &txs[1])
    }));

    // remove stale
    mp_arc.lock().unwrap().remove_stale(|tx| !tx.hash().eq(&txs[2].hash()), &fs);
    assert!(wait_for_event(&sub_rx1, Duration::from_secs(1), Duration::from_millis(100), |event| {
        matches!(event, Event::TransactionRemoved { tx } if tx == &txs[2])
    }));
    assert!(wait_for_event(&sub_rx2, Duration::from_secs(1), Duration::from_millis(100), |event| {
        matches!(event, Event::TransactionRemoved { tx } if tx == &txs[2])
    }));

    // unsubscribe
    mp_arc.lock().unwrap().unsubscribe_from_transactions(&sub_tx1);
    assert!(mp_arc.lock().unwrap().add(&txs[3], &fs).is_ok());
    assert!(wait_for_event(&sub_rx2, Duration::from_secs(1), Duration::from_millis(100), |event| {
        matches!(event, Event::TransactionAdded { tx } if tx == &txs[3])
    }));
    assert_eq!(sub_rx1.try_recv().is_err(), true);

    handle.join().unwrap();
}

fn wait_for_event<F>(rx: &Receiver<Event>, timeout: Duration, interval: Duration, predicate: F) -> bool
where
    F: Fn(&Event) -> bool,
{
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if let Ok(event) = rx.try_recv() {
            if predicate(&event) {
                return true;
            }
        }
        thread::sleep(interval);
    }
    false
}
