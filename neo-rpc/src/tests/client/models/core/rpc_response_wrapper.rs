use super::*;
use neo_primitives::UInt256;

#[test]
fn test_unconfirmed_response() {
    let data = "test_transaction".to_string();
    let response = RpcResponse::unconfirmed(data);

    assert!(!response.is_confirmed());
    assert_eq!(response.confirmations(), 0);
    assert!(response.block_hash.is_none());
}

#[test]
fn test_confirmed_response() {
    let data = "test_transaction".to_string();
    let block_hash = UInt256::zero();
    let response = RpcResponse::confirmed(data, block_hash, 10, 1234567890);

    assert!(response.is_confirmed());
    assert_eq!(response.confirmations(), 10);
    assert_eq!(response.block_time, Some(1234567890));
}

#[test]
fn test_with_vm_state() {
    let data = "test".to_string();
    let response = RpcResponse::unconfirmed(data).with_vm_state("HALT".to_string());

    assert_eq!(response.vm_state, Some("HALT".to_string()));
}

#[test]
fn test_map() {
    let data = 42i32;
    let response = RpcResponse::unconfirmed(data);
    let mapped = response.map(|n| n.to_string());

    assert_eq!(mapped.data, "42");
}

#[test]
fn test_deref() {
    let data = vec![1, 2, 3];
    let response = RpcResponse::unconfirmed(data);

    assert_eq!(response.len(), 3); // Uses Deref to access Vec methods
    assert_eq!(response[0], 1);
}
