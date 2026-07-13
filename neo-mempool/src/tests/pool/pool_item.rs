use super::PoolItem;
use neo_payloads::{Signer, Transaction, TransactionAttribute, Witness};
use neo_primitives::{UInt160, WitnessScope};
use neo_vm::OpCode;
use std::cmp::Ordering;

fn make_transaction(nonce: u32, network_fee: i64, high_priority: bool) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(nonce);
    tx.set_network_fee(network_fee);
    tx.set_script(vec![OpCode::RET.byte()]);
    tx.set_signers(vec![Signer::new(UInt160::zero(), WitnessScope::NONE)]);
    tx.set_witnesses(vec![Witness::empty()]);
    if high_priority {
        tx.set_attributes(vec![TransactionAttribute::high_priority()]);
    }
    tx
}

#[test]
fn pool_item_compare_orders_by_fee() {
    let tx1 = make_transaction(1, 1, false);
    let tx2 = make_transaction(2, 2, false);
    let item1 = PoolItem::new(tx1);
    let item2 = PoolItem::new(tx2);
    assert_eq!(item1.compare_to(&item2), Ordering::Less);
    assert_eq!(item2.compare_to(&item1), Ordering::Greater);
}

#[test]
fn pool_item_compare_respects_high_priority() {
    let low = PoolItem::new(make_transaction(3, 1, false));
    let high = PoolItem::new(make_transaction(4, 1, true));
    assert_eq!(low.compare_to(&high), Ordering::Less);
    assert_eq!(high.compare_to(&low), Ordering::Greater);
}

#[test]
fn pool_item_compare_orders_by_hash_descending() {
    let tx1 = make_transaction(5, 1, false);
    let tx2 = make_transaction(6, 1, false);
    let item1 = PoolItem::new(tx1.clone());
    let item2 = PoolItem::new(tx2.clone());
    let expected = if tx1.hash() > tx2.hash() {
        Ordering::Less
    } else if tx1.hash() < tx2.hash() {
        Ordering::Greater
    } else {
        Ordering::Equal
    };
    assert_eq!(item1.compare_to(&item2), expected);
}
