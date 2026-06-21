use super::*;
use neo_primitives::UInt256;

#[test]
fn confirm_and_contains() {
    let mut ctx = TransactionVerificationContext::new();
    let h = UInt256::from([42u8; 32]);
    assert!(!ctx.contains(&h));
    assert!(ctx.confirm(h));
    assert!(ctx.contains(&h));
    // Re-confirming the same hash is idempotent.
    assert!(!ctx.confirm(h));
}

#[test]
fn rotate_promotes_confirmed_to_historic() {
    let mut ctx = TransactionVerificationContext::new();
    let h = UInt256::from([7u8; 32]);
    ctx.confirm(h);
    ctx.rotate();
    assert!(ctx.confirmed.is_empty());
    assert!(ctx.historic.contains(&h));
    assert!(ctx.contains(&h));
}
