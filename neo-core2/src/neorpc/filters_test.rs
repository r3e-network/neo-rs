use crate::neorpc::{BlockFilter, TxFilter, NotificationFilter, ExecutionFilter};
use crate::util::{Uint160, Uint256};
use std::ptr;
use std::mem;

#[test]
fn test_block_filter_copy() {
    let mut bf: Option<BlockFilter> = None;
    let mut tf: Option<BlockFilter>;

    assert!(bf.is_none());

    bf = Some(BlockFilter::default());
    tf = Some(bf.as_ref().unwrap().clone());
    assert_eq!(bf, tf);

    bf.as_mut().unwrap().primary = Some(42);

    tf = Some(bf.as_ref().unwrap().clone());
    assert_eq!(bf, tf);
    bf.as_mut().unwrap().primary = Some(100);
    assert_ne!(bf, tf);
    
    bf.as_mut().unwrap().since = Some(42);

    tf = Some(bf.as_ref().unwrap().clone());
    assert_eq!(bf, tf);
    bf.as_mut().unwrap().since = Some(100500);
    assert_ne!(bf, tf);

    bf.as_mut().unwrap().till = Some(42);

    tf = Some(bf.as_ref().unwrap().clone());
    assert_eq!(bf, tf);
    bf.as_mut().unwrap().till = Some(100500);
    assert_ne!(bf, tf);
}

#[test]
fn test_tx_filter_copy() {
    let mut bf: Option<TxFilter> = None;
    let mut tf: Option<TxFilter>;

    assert!(bf.is_none());

    bf = Some(TxFilter::default());
    tf = Some(bf.as_ref().unwrap().clone());
    assert_eq!(bf, tf);

    bf.as_mut().unwrap().sender = Some(Uint160::from([1, 2, 3]));

    tf = Some(bf.as_ref().unwrap().clone());
    assert_eq!(bf, tf);
    bf.as_mut().unwrap().sender = Some(Uint160::from([3, 2, 1]));
    assert_ne!(bf, tf);

    bf.as_mut().unwrap().signer = Some(Uint160::from([1, 2, 3]));

    tf = Some(bf.as_ref().unwrap().clone());
    assert_eq!(bf, tf);
    bf.as_mut().unwrap().signer = Some(Uint160::from([3, 2, 1]));
    assert_ne!(bf, tf);
}

#[test]
fn test_notification_filter_copy() {
    let mut bf: Option<NotificationFilter> = None;
    let mut tf: Option<NotificationFilter>;

    assert!(bf.is_none());

    bf = Some(NotificationFilter::default());
    tf = Some(bf.as_ref().unwrap().clone());
    assert_eq!(bf, tf);

    bf.as_mut().unwrap().contract = Some(Uint160::from([1, 2, 3]));

    tf = Some(bf.as_ref().unwrap().clone());
    assert_eq!(bf, tf);
    bf.as_mut().unwrap().contract = Some(Uint160::from([3, 2, 1]));
    assert_ne!(bf, tf);

    bf.as_mut().unwrap().name = Some("ololo".to_string());

    tf = Some(bf.as_ref().unwrap().clone());
    assert_eq!(bf, tf);
    bf.as_mut().unwrap().name = Some("azaza".to_string());
    assert_ne!(bf, tf);
}

#[test]
fn test_execution_filter_copy() {
    let mut bf: Option<ExecutionFilter> = None;
    let mut tf: Option<ExecutionFilter>;

    assert!(bf.is_none());

    bf = Some(ExecutionFilter::default());
    tf = Some(bf.as_ref().unwrap().clone());
    assert_eq!(bf, tf);

    bf.as_mut().unwrap().state = Some("ololo".to_string());

    tf = Some(bf.as_ref().unwrap().clone());
    assert_eq!(bf, tf);
    bf.as_mut().unwrap().state = Some("azaza".to_string());
    assert_ne!(bf, tf);

    bf.as_mut().unwrap().container = Some(Uint256::from([1, 2, 3]));

    tf = Some(bf.as_ref().unwrap().clone());
    assert_eq!(bf, tf);
    bf.as_mut().unwrap().container = Some(Uint256::from([3, 2, 1]));
    assert_ne!(bf, tf);
}
