use super::*;

#[test]
fn test_size_constants() {
    assert_eq!(ADDRESS_SIZE, 20);
    assert_eq!(HASH_SIZE, 32);
    assert_eq!(ONE_MEGABYTE, 1_048_576);
    assert_eq!(ONE_KILOBYTE, 1024);
}

#[test]
fn test_block_constants() {
    assert_eq!(MAX_BLOCK_SIZE, 2_097_152); // 2MB
    assert_eq!(MAX_TRANSACTIONS_PER_BLOCK, 512);
    assert_eq!(MAX_TRACEABLE_BLOCKS, 2_102_400);
}

#[test]
fn test_transaction_constants() {
    assert_eq!(MAX_TRANSACTION_SIZE, 102_400); // 100KB
    assert_eq!(MAX_TRANSACTION_ATTRIBUTES, 16);
    assert_eq!(MAX_COSIGNERS, 16);
}

#[test]
fn test_time_constants() {
    assert_eq!(SECONDS_PER_BLOCK, 15);
    assert_eq!(MILLISECONDS_PER_BLOCK, 15_000);
    assert_eq!(MILLISECONDS_PER_HOUR, 3_600_000);
}

#[test]
fn test_network_magic() {
    assert_eq!(TESTNET_MAGIC, 0x3554_334E);
    assert_eq!(MAINNET_MAGIC, 0x334F_454E);
}

#[test]
fn test_port_constants() {
    assert_eq!(TESTNET_RPC_PORT, 20332);
    assert_eq!(TESTNET_P2P_PORT, 20333);
    assert_eq!(MAINNET_RPC_PORT, 10332);
    assert_eq!(MAINNET_P2P_PORT, 10333);
}

#[test]
fn test_fee_constants() {
    assert_eq!(GAS_PER_BYTE, 1000);
    assert_eq!(MIN_NETWORK_FEE, 100_000);
}

#[test]
fn test_consensus_constants() {
    assert_eq!(MAX_VALIDATORS, 21);
    assert_eq!(MIN_VALIDATORS, 4);
}

#[test]
fn test_vm_constants() {
    assert_eq!(MAX_STACK_SIZE, 2048);
    assert_eq!(MAX_INVOCATION_STACK_SIZE, 1024);
}
