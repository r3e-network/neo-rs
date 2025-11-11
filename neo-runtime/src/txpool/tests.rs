use super::{PendingTransaction, TxPool};

#[test]
fn insert_and_reserve() {
    let mut pool = TxPool::new();
    assert!(pool.insert(PendingTransaction::new("tx1", 10, 100)));
    assert!(pool.insert(PendingTransaction::new("tx2", 20, 150)));
    assert!(!pool.insert(PendingTransaction::new("tx1", 5, 50)));

    let reserved = pool.reserve_for_block(10, 120);
    assert_eq!(reserved.len(), 1);
    assert_eq!(reserved[0].id, "tx1");
    assert!(pool.contains("tx2"));
}

#[test]
fn remove_eliminates_from_queue() {
    let mut pool = TxPool::new();
    pool.insert(PendingTransaction::new("tx1", 10, 100));
    pool.insert(PendingTransaction::new("tx2", 20, 150));
    assert!(pool.remove("tx1").is_some());
    assert!(!pool.contains("tx1"));
    let reserved = pool.reserve_for_block(5, 1_000);
    assert_eq!(reserved.len(), 1);
    assert_eq!(reserved[0].id, "tx2");
}
