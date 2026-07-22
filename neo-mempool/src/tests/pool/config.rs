use super::*;

#[test]
fn default_capacity_matches_the_operator_default() {
    let config = TxPoolConfig::default();

    assert_eq!(config.max_transactions(), DEFAULT_MAX_TRANSACTIONS);
}

#[test]
fn zero_capacity_is_rejected() {
    assert_eq!(TxPoolConfig::new(0), Err(TxPoolConfigError::ZeroCapacity));
}

#[test]
fn explicit_capacity_is_preserved() {
    let config = TxPoolConfig::new(321).expect("positive capacity");

    assert_eq!(config.max_transactions(), 321);
}
