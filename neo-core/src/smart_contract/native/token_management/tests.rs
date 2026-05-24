use super::*;
use neo_vm_rs::StackValue;

fn sample_uint160(seed: u8) -> UInt160 {
    UInt160::from_bytes(&[seed; 20]).expect("valid UInt160")
}

#[test]
fn token_state_projects_to_stack_value() {
    let owner = sample_uint160(1);
    let state = TokenState {
        token_type: TokenType::NonFungible,
        owner,
        name: "Token".to_string(),
        symbol: "TKN".to_string(),
        decimals: 8,
        total_supply: BigInt::from(123_456),
        max_supply: BigInt::from(999_999),
        mintable_address: Some(owner),
    };

    assert_eq!(
        state.to_stack_value(),
        StackValue::Struct(vec![
            StackValue::Integer(1),
            StackValue::ByteString(owner.to_bytes()),
            StackValue::ByteString(b"Token".to_vec()),
            StackValue::ByteString(b"TKN".to_vec()),
            StackValue::Integer(8),
            StackValue::BigInteger(BigInt::from(123_456).to_signed_bytes_le()),
            StackValue::BigInteger(BigInt::from(999_999).to_signed_bytes_le()),
            StackValue::Boolean(true),
        ])
    );
}

#[test]
fn token_state_reads_stack_value() {
    let owner = sample_uint160(2);
    let mut state = TokenState::default();

    state
        .from_stack_value(StackValue::Struct(vec![
            StackValue::Integer(0),
            StackValue::ByteString(owner.to_bytes()),
            StackValue::ByteString(b"Name".to_vec()),
            StackValue::ByteString(b"SYM".to_vec()),
            StackValue::Integer(9),
            StackValue::BigInteger(BigInt::from(42).to_signed_bytes_le()),
            StackValue::BigInteger(BigInt::from(84).to_signed_bytes_le()),
            StackValue::Boolean(true),
        ]))
        .expect("parse token state");

    assert_eq!(state.token_type, TokenType::Fungible);
    assert_eq!(state.owner, owner);
    assert_eq!(state.name, "Name");
    assert_eq!(state.symbol, "SYM");
    assert_eq!(state.decimals, 9);
    assert_eq!(state.total_supply, BigInt::from(42));
    assert_eq!(state.max_supply, BigInt::from(84));
    assert_eq!(state.mintable_address, Some(owner));
}

#[test]
fn account_state_projects_to_stack_value() {
    let state = AccountState::with_balance(BigInt::from(77));

    assert_eq!(
        state.to_stack_value(),
        StackValue::Struct(vec![StackValue::BigInteger(
            BigInt::from(77).to_signed_bytes_le()
        )])
    );
}

#[test]
fn account_state_reads_stack_value() {
    let mut state = AccountState::default();

    state
        .from_stack_value(StackValue::Struct(vec![StackValue::BigInteger(
            BigInt::from(88).to_signed_bytes_le(),
        )]))
        .expect("parse account state");

    assert_eq!(state.balance, BigInt::from(88));
}

#[test]
fn nft_state_projects_to_stack_value() {
    let asset_id = sample_uint160(3);
    let owner = sample_uint160(4);
    let state = NFTState {
        asset_id,
        owner,
        properties: vec![(b"name".to_vec(), b"value".to_vec())],
    };

    assert_eq!(
        state.to_stack_value(),
        StackValue::Struct(vec![
            StackValue::ByteString(asset_id.to_bytes()),
            StackValue::ByteString(owner.to_bytes()),
            StackValue::Array(vec![StackValue::Struct(vec![
                StackValue::ByteString(b"name".to_vec()),
                StackValue::ByteString(b"value".to_vec()),
            ])]),
        ])
    );
}

#[test]
fn nft_state_reads_stack_value() {
    let asset_id = sample_uint160(5);
    let owner = sample_uint160(6);
    let mut state = NFTState::default();

    state
        .from_stack_value(StackValue::Struct(vec![
            StackValue::ByteString(asset_id.to_bytes()),
            StackValue::ByteString(owner.to_bytes()),
            StackValue::Array(vec![
                StackValue::Struct(vec![
                    StackValue::ByteString(b"color".to_vec()),
                    StackValue::ByteString(b"blue".to_vec()),
                ]),
                StackValue::Boolean(true),
            ]),
        ]))
        .expect("parse nft state");

    assert_eq!(state.asset_id, asset_id);
    assert_eq!(state.owner, owner);
    assert_eq!(
        state.properties,
        vec![(b"color".to_vec(), b"blue".to_vec())]
    );
}

#[test]
fn test_token_state_default() {
    let state = TokenState::default();
    assert_eq!(state.token_type, TokenType::Fungible);
    assert_eq!(state.total_supply, BigInt::from(0));
}

#[test]
fn test_account_state_new() {
    let state = AccountState::new();
    assert_eq!(state.balance, BigInt::from(0));
}

#[test]
fn test_nft_state_new() {
    let nft = NFTState::new();
    assert!(nft.properties.is_empty());
}
