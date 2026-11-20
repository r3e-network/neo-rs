//! Unit tests for TokenManagement state projections and Boolean argument
//! conversion.
//!
//! Migrated from `neo-core/src/smart_contract/native/token_management/tests.rs`.
//! The state-projection test bodies are unchanged; `use super::*` continues to
//! pull the state types (`TokenState`, `AccountState`, `NFTState`, `TokenType`),
//! `UInt160`, and `BigInt` into scope from the parent `token_management` module.
//! The `mintable` tests at the end of the file invoke `create` and
//! `createNonFungible` through `invoke_method` and read the stored token state.

use super::*;
use neo_primitives::TriggerType;
use neo_storage::persistence::PersistenceDataCache;
use neo_vm::StackValue;
use std::sync::Arc;

fn sample_uint160(seed: u8) -> UInt160 {
    UInt160::from_bytes(&[seed; 20]).expect("valid UInt160")
}

/// Builds an engine with TokenManagement attached to that engine only; the
/// contract is deliberately absent from the standard registry.
fn make_engine() -> ApplicationEngine {
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        None,
        Arc::new(PersistenceDataCache::new(false)),
        None,
        ProtocolSettings::default(),
        400_000_000,
        None,
        crate::native::composition::default_native_services(),
    )
    .expect("engine");
    engine.register_native_contract(Arc::new(TokenManagement::new()));
    engine
}

/// Raw `mintable` argument bytes paired with the Boolean Neo derives from them.
fn mintable_cases() -> Vec<(&'static str, Vec<u8>, bool)> {
    vec![
        ("empty", vec![], false),
        ("Integer(0)", vec![0], false),
        ("two zero bytes", vec![0, 0], false),
        ("Integer(1)", vec![1], true),
        ("Integer(256)", vec![0, 1], true),
        ("Integer(-1)", BigInt::from(-1).to_signed_bytes_le(), true),
        (
            "Integer(-256)",
            BigInt::from(-256).to_signed_bytes_le(),
            true,
        ),
    ]
}

/// Reads back the token state persisted by a create call that returned `result`.
fn stored_token_state(
    contract: &TokenManagement,
    engine: &ApplicationEngine,
    result: &[u8],
) -> TokenState {
    let asset_id = UInt160::from_bytes(result).expect("asset id");
    contract
        .get_token_state(engine, &asset_id)
        .expect("read token state")
        .expect("token state stored")
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
fn token_state_default() {
    let state = TokenState::default();
    assert_eq!(state.token_type, TokenType::Fungible);
    assert_eq!(state.total_supply, BigInt::from(0));
}

#[test]
fn account_state_new() {
    let state = AccountState::new();
    assert_eq!(state.balance, BigInt::from(0));
}

#[test]
fn nft_state_new() {
    let nft = NFTState::new();
    assert!(nft.properties.is_empty());
}

#[test]
fn create_reads_mintable_as_neo_boolean() {
    let contract = TokenManagement::new();
    let owner = sample_uint160(7);

    for (label, mintable, expected) in mintable_cases() {
        let mut engine = make_engine();
        let args = vec![
            vec![0],
            owner.to_bytes(),
            b"Token".to_vec(),
            b"TKN".to_vec(),
            vec![8],
            vec![],
            mintable,
        ];

        let result = contract
            .invoke_method(&mut engine, "create", &args)
            .unwrap_or_else(|err| panic!("create with mintable {label}: {err}"));
        let state = stored_token_state(&contract, &engine, &result);

        assert_eq!(state.token_type, TokenType::Fungible, "mintable {label}");
        assert_eq!(
            state.mintable_address,
            expected.then_some(owner),
            "mintable {label}"
        );
    }
}

#[test]
fn create_non_fungible_reads_mintable_as_neo_boolean() {
    let contract = TokenManagement::new();
    let owner = sample_uint160(8);

    for (label, mintable, expected) in mintable_cases() {
        let mut engine = make_engine();
        let args = vec![
            owner.to_bytes(),
            b"Collectible".to_vec(),
            b"NFT".to_vec(),
            mintable,
        ];

        let result = contract
            .invoke_method(&mut engine, "createNonFungible", &args)
            .unwrap_or_else(|err| panic!("createNonFungible with mintable {label}: {err}"));
        let state = stored_token_state(&contract, &engine, &result);

        assert_eq!(state.token_type, TokenType::NonFungible, "mintable {label}");
        assert_eq!(
            state.mintable_address,
            expected.then_some(owner),
            "mintable {label}"
        );
    }
}
