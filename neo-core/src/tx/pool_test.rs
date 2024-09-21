// Copyright @ 2023 - 2024, R3E Network
// All Rights Reserved

use crate::tx::TxPool;

#[test]
fn test_txpool_basic() {
    let pool = TxPool::new(1024, 0);
    let txs = pool.get_verified_txs(0);
    assert_eq!(txs.len(), 0);
}
