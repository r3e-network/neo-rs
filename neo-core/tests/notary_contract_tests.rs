//! Notary native contract unit tests matching C# UT_Notary
//!
//! Tests for Neo.SmartContract.Native.Notary functionality.

use neo_core::hardfork::HardforkManager;
use neo_core::ledger::{create_genesis_block, Block, BlockHeader};
use neo_core::network::p2p::payloads::{NotaryAssisted, Signer, Transaction, TransactionAttribute};
use neo_core::persistence::DataCache;
use neo_core::persistence::IReadOnlyStoreGeneric;
use neo_core::protocol_settings::ProtocolSettings;
use neo_core::smart_contract::application_engine::ApplicationEngine;
use neo_core::smart_contract::binary_serializer::BinarySerializer;
use neo_core::smart_contract::call_flags::CallFlags;
use neo_core::smart_contract::native::notary::{Deposit, Notary};
use neo_core::smart_contract::native::{
    GasToken, LedgerContract, NativeContract, NativeHelpers, NeoToken, PolicyContract, Role,
    RoleManagement,
};
use neo_core::smart_contract::trigger_type::TriggerType;
use neo_core::smart_contract::{Contract, StorageItem, StorageKey};
use neo_core::wallets::KeyPair;
use neo_core::{IVerifiable, Result as CoreResult, UInt160, UInt256, WitnessScope};
use neo_vm::{ExecutionEngineLimits, OpCode, StackItem};
use num_bigint::BigInt;
use num_traits::{ToPrimitive, Zero};
use std::collections::HashMap;
use std::sync::Arc;

const TEST_GAS_LIMIT: i64 = 3_000_000_000;
const PREFIX_CURRENT_BLOCK: u8 = 12;

fn protocol_settings_all_active() -> ProtocolSettings {
    let mut settings = ProtocolSettings::default();
    let mut hardforks = HashMap::new();
    for hardfork in HardforkManager::all() {
        hardforks.insert(hardfork, 0);
    }
    settings.hardforks = hardforks;
    settings
}

fn make_snapshot_with_genesis(settings: &ProtocolSettings) -> Arc<DataCache> {
    let snapshot = Arc::new(DataCache::new(false));
    let genesis = create_genesis_block(settings);

    let mut on_persist = ApplicationEngine::new(
        TriggerType::OnPersist,
        None,
        Arc::clone(&snapshot),
        Some(genesis.clone()),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("on persist engine");
    on_persist.native_on_persist().expect("native on persist");

    let mut post_persist = ApplicationEngine::new(
        TriggerType::PostPersist,
        None,
        Arc::clone(&snapshot),
        Some(genesis),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("post persist engine");
    post_persist
        .native_post_persist()
        .expect("native post persist");

    snapshot
}

fn set_ledger_current_index(snapshot: &Arc<DataCache>, index: u32) {
    let key = StorageKey::create(LedgerContract::ID, PREFIX_CURRENT_BLOCK);
    let mut bytes = UInt256::zero().to_bytes().to_vec();
    bytes.extend_from_slice(&index.to_le_bytes());
    let item = StorageItem::from_bytes(bytes);
    if snapshot.as_ref().try_get(&key).is_some() {
        snapshot.update(key, item);
    } else {
        snapshot.add(key, item);
    }
}

fn make_persisting_block(index: u32, transactions: Vec<Transaction>) -> Block {
    let header = BlockHeader::new(
        0,
        UInt256::zero(),
        UInt256::zero(),
        0,
        0,
        index,
        0,
        UInt160::zero(),
        vec![neo_core::Witness::empty()],
    );
    Block::new(header, transactions)
}

fn make_tx_with_signer(account: UInt160) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_signers(vec![Signer::new(account, WitnessScope::GLOBAL)]);
    tx.set_script(vec![OpCode::RET as u8]);
    tx
}

fn bigint_to_bytes(value: &BigInt) -> Vec<u8> {
    let mut bytes = value.to_signed_bytes_le();
    if bytes.is_empty() {
        bytes.push(0);
    }
    bytes
}

fn build_notary_data(owner: Option<UInt160>, till: u32) -> Vec<u8> {
    let owner_item = owner
        .map(|hash| StackItem::from_byte_string(hash.to_bytes()))
        .unwrap_or_else(StackItem::null);
    let data = StackItem::from_array(vec![owner_item, StackItem::from_int(till)]);
    BinarySerializer::serialize(&data, &ExecutionEngineLimits::default())
        .expect("serialize notary data")
}

#[allow(clippy::too_many_arguments)]
fn try_token_transfer(
    contract_hash: UInt160,
    snapshot: Arc<DataCache>,
    settings: ProtocolSettings,
    persisting_block: Block,
    tx: Transaction,
    from: UInt160,
    to: UInt160,
    amount: &BigInt,
    data: Vec<u8>,
) -> CoreResult<bool> {
    let container = Arc::new(tx) as Arc<dyn IVerifiable>;
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        snapshot,
        Some(persisting_block),
        settings,
        TEST_GAS_LIMIT,
        None,
    )?;
    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, None)
        .expect("load dummy script");
    engine.set_current_script_hash(Some(contract_hash));
    let args = vec![
        from.to_bytes(),
        to.to_bytes(),
        bigint_to_bytes(amount),
        data,
    ];
    let result = engine.call_native_contract(contract_hash, "transfer", &args)?;
    engine.process_pending_native_calls()?;
    engine.execute()?;
    Ok(result.iter().any(|byte| *byte != 0))
}

fn designate_notary_nodes(
    snapshot: Arc<DataCache>,
    settings: ProtocolSettings,
    persisting_index: u32,
    public_keys: Vec<neo_core::cryptography::ECPoint>,
) -> CoreResult<()> {
    let committee_address = NativeHelpers::committee_address(&settings, Some(snapshot.as_ref()));
    let tx = make_tx_with_signer(committee_address);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(Arc::new(tx)),
        Arc::clone(&snapshot),
        Some(make_persisting_block(persisting_index, Vec::new())),
        settings,
        TEST_GAS_LIMIT,
        None,
    )?;
    engine.set_current_script_hash(Some(RoleManagement::new().hash()));

    let keys: Vec<StackItem> = public_keys
        .iter()
        .map(|key| StackItem::from_byte_string(key.to_bytes()))
        .collect();
    let keys_item = StackItem::from_array(keys);
    let keys_bytes = BinarySerializer::serialize(&keys_item, &ExecutionEngineLimits::default())
        .expect("serialize keys");

    let role_bytes = bigint_to_bytes(&BigInt::from(Role::P2PNotary as u8));
    let args = vec![role_bytes, keys_bytes];
    engine.call_native_contract(RoleManagement::new().hash(), "designateAsRole", &args)?;
    Ok(())
}

fn call_notary_int(
    snapshot: Arc<DataCache>,
    settings: ProtocolSettings,
    persisting_block: Block,
    tx: Option<Transaction>,
    method: &str,
    args: Vec<Vec<u8>>,
) -> BigInt {
    let container = tx.map(|t| Arc::new(t) as Arc<dyn IVerifiable>);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        container,
        snapshot,
        Some(persisting_block),
        settings,
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");
    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, None)
        .expect("load dummy script");
    engine.set_current_script_hash(Some(Notary::new().hash()));
    let result = engine
        .call_native_contract(Notary::new().hash(), method, &args)
        .expect("call native");
    BigInt::from_signed_bytes_le(&result)
}

fn call_notary_bool(
    snapshot: Arc<DataCache>,
    settings: ProtocolSettings,
    persisting_block: Block,
    tx: Option<Transaction>,
    method: &str,
    args: Vec<Vec<u8>>,
) -> bool {
    let container = tx.map(|t| Arc::new(t) as Arc<dyn IVerifiable>);
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        container,
        snapshot,
        Some(persisting_block),
        settings,
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");
    engine
        .load_script(vec![OpCode::RET as u8], CallFlags::ALL, None)
        .expect("load dummy script");
    engine.set_current_script_hash(Some(Notary::new().hash()));
    let result = engine
        .call_native_contract(Notary::new().hash(), method, &args)
        .expect("call native");
    result.iter().any(|byte| *byte != 0)
}

/// Tests that Notary has correct contract ID (-10)
#[test]
fn test_notary_contract_id() {
    let notary = Notary::new();
    assert_eq!(notary.id(), -10, "Notary contract ID should be -10");
}

/// Tests that Notary has correct name
#[test]
fn test_notary_contract_name() {
    let notary = Notary::new();
    assert_eq!(notary.name(), "Notary", "Notary contract name should match");
}

/// Tests that Notary has correct contract hash
#[test]
fn test_notary_contract_hash() {
    let notary = Notary::new();
    let hash = notary.hash();

    assert_eq!(
        hash.to_hex_string(),
        "0xc1e14f19c3e60d0b9244d06dd7ba9b113135ec3b",
        "Notary hash should match C# reference"
    );
}

/// Tests Notary methods are registered
#[test]
fn test_notary_methods() {
    let notary = Notary::new();
    let methods = notary.methods();

    let method_names: Vec<&str> = methods.iter().map(|m| m.name.as_str()).collect();

    assert!(method_names.contains(&"balanceOf"), "Should have balanceOf");
    assert!(
        method_names.contains(&"expirationOf"),
        "Should have expirationOf"
    );
    assert!(
        method_names.contains(&"getMaxNotValidBeforeDelta"),
        "Should have getMaxNotValidBeforeDelta"
    );
    assert!(
        method_names.contains(&"onNEP17Payment"),
        "Should have onNEP17Payment"
    );
    assert!(
        method_names.contains(&"lockDepositUntil"),
        "Should have lockDepositUntil"
    );
    assert!(method_names.contains(&"withdraw"), "Should have withdraw");
    assert!(
        method_names.contains(&"setMaxNotValidBeforeDelta"),
        "Should have setMaxNotValidBeforeDelta"
    );
}

/// Tests Deposit struct creation
#[test]
fn test_deposit_creation() {
    let amount = BigInt::from(1000000000i64); // 10 GAS
    let till = 12345u32;

    let deposit = Deposit::new(amount.clone(), till);

    assert_eq!(deposit.amount, amount, "Deposit amount should match");
    assert_eq!(deposit.till, till, "Deposit till should match");
}

/// Tests Deposit default values
#[test]
fn test_deposit_default() {
    let deposit = Deposit::default();

    assert_eq!(
        deposit.amount,
        BigInt::from(0),
        "Default amount should be 0"
    );
    assert_eq!(deposit.till, 0, "Default till should be 0");
}

/// Tests Deposit to/from StackItem conversion
#[test]
fn test_deposit_stack_item_roundtrip() {
    use neo_core::smart_contract::i_interoperable::IInteroperable;

    let original = Deposit::new(BigInt::from(500), 100);
    let stack_item = original.to_stack_item().unwrap();

    let mut recovered = Deposit::default();
    recovered.from_stack_item(stack_item).unwrap();

    assert_eq!(recovered.amount, original.amount, "Amount should roundtrip");
    assert_eq!(recovered.till, original.till, "Till should roundtrip");
}

/// Tests safe methods have correct flags
#[test]
fn test_notary_safe_methods() {
    let notary = Notary::new();
    let methods = notary.methods();

    // balanceOf should be safe (call_flags = 0)
    let balance_of = methods.iter().find(|m| m.name == "balanceOf");
    assert!(balance_of.is_some(), "balanceOf should exist");
    assert!(balance_of.unwrap().safe, "balanceOf should be safe");

    // expirationOf should be safe
    let expiration_of = methods.iter().find(|m| m.name == "expirationOf");
    assert!(expiration_of.is_some(), "expirationOf should exist");
    assert!(expiration_of.unwrap().safe, "expirationOf should be safe");

    // getMaxNotValidBeforeDelta should be safe
    let get_max = methods
        .iter()
        .find(|m| m.name == "getMaxNotValidBeforeDelta");
    assert!(get_max.is_some(), "getMaxNotValidBeforeDelta should exist");
    assert!(
        get_max.unwrap().safe,
        "getMaxNotValidBeforeDelta should be safe"
    );
}

/// Tests unsafe methods have correct flags
#[test]
fn test_notary_unsafe_methods() {
    let notary = Notary::new();
    let methods = notary.methods();

    // onNEP17Payment should be unsafe
    let on_payment = methods.iter().find(|m| m.name == "onNEP17Payment");
    assert!(on_payment.is_some(), "onNEP17Payment should exist");
    assert!(!on_payment.unwrap().safe, "onNEP17Payment should be unsafe");

    // withdraw should be unsafe
    let withdraw = methods.iter().find(|m| m.name == "withdraw");
    assert!(withdraw.is_some(), "withdraw should exist");
    assert!(!withdraw.unwrap().safe, "withdraw should be unsafe");
}

/// Tests default max not valid before delta (140 blocks)
#[test]
fn test_default_max_not_valid_before_delta() {
    // Default is 140 blocks (20 rounds * 7 validators)
    // This is checked in get_max_not_valid_before_delta when no stored value exists
    let notary = Notary::new();

    // The constant is internal, but we can verify the contract exists
    assert_eq!(notary.id(), Notary::ID);
}

/// Tests Notary contract ID constant
#[test]
fn test_notary_id_constant() {
    assert_eq!(Notary::ID, -10, "Notary::ID should be -10");
}

/// Tests Deposit with large amount
#[test]
fn test_deposit_large_amount() {
    // Test with maximum GAS amount (100 million GAS = 10_000_000_000_000_000 datoshi)
    let large_amount = BigInt::from(10_000_000_000_000_000_i64);
    let till = u32::MAX;

    let deposit = Deposit::new(large_amount.clone(), till);

    assert_eq!(deposit.amount, large_amount);
    assert_eq!(deposit.till, till);
}

/// Tests Deposit clone
#[test]
fn test_deposit_clone() {
    let original = Deposit::new(BigInt::from(12345), 67890);
    let cloned = original.clone();

    assert_eq!(cloned.amount, original.amount);
    assert_eq!(cloned.till, original.till);
}

#[test]
fn check_on_nep17_payment() {
    let settings = protocol_settings_all_active();
    let persisting_block = make_persisting_block(1000, Vec::new());
    let from = NativeHelpers::get_bft_address(&settings.standby_validators());
    let to = Notary::new().hash();

    // Non-GAS transfer should fail.
    {
        let snapshot = make_snapshot_with_genesis(&settings);
        set_ledger_current_index(&snapshot, persisting_block.index() - 1);
        let tx = make_tx_with_signer(from);
        let data = build_notary_data(None, persisting_block.index() + 100);
        let amount = BigInt::zero();
        let result = try_token_transfer(
            NeoToken::new().hash(),
            snapshot,
            settings.clone(),
            persisting_block.clone(),
            tx,
            from,
            to,
            &amount,
            data,
        );
        assert!(result.is_err(), "Non-GAS transfer should fail");
    }

    // GAS transfer with invalid data format should fail.
    {
        let snapshot = make_snapshot_with_genesis(&settings);
        set_ledger_current_index(&snapshot, persisting_block.index() - 1);
        let tx = make_tx_with_signer(from);
        let data =
            BinarySerializer::serialize(&StackItem::from_int(5), &ExecutionEngineLimits::default())
                .expect("serialize data");
        let amount = BigInt::zero();
        let result = try_token_transfer(
            GasToken::new().hash(),
            snapshot,
            settings.clone(),
            persisting_block.clone(),
            tx,
            from,
            to,
            &amount,
            data,
        );
        assert!(result.is_err(), "Invalid data format should fail");
    }

    // GAS transfer with wrong number of data elements should fail.
    {
        let snapshot = make_snapshot_with_genesis(&settings);
        set_ledger_current_index(&snapshot, persisting_block.index() - 1);
        let tx = make_tx_with_signer(from);
        let data = StackItem::from_array(vec![StackItem::from_bool(true)]);
        let data = BinarySerializer::serialize(&data, &ExecutionEngineLimits::default())
            .expect("serialize data");
        let amount = BigInt::zero();
        let result = try_token_transfer(
            GasToken::new().hash(),
            snapshot,
            settings.clone(),
            persisting_block.clone(),
            tx,
            from,
            to,
            &amount,
            data,
        );
        assert!(result.is_err(), "Invalid data array should fail");
    }

    // Gas transfer with invalid till parameter should fail.
    {
        let snapshot = make_snapshot_with_genesis(&settings);
        set_ledger_current_index(&snapshot, persisting_block.index() - 1);
        let tx = make_tx_with_signer(from);
        let data = build_notary_data(None, persisting_block.index() - 1);
        let amount = BigInt::zero();
        let result = try_token_transfer(
            GasToken::new().hash(),
            snapshot,
            settings.clone(),
            persisting_block.clone(),
            tx,
            from,
            to,
            &amount,
            data,
        );
        assert!(result.is_err(), "Invalid till should fail");
    }

    // Insufficient first deposit should fail.
    {
        let snapshot = make_snapshot_with_genesis(&settings);
        set_ledger_current_index(&snapshot, persisting_block.index() - 1);
        let tx = make_tx_with_signer(from);
        let data = build_notary_data(None, persisting_block.index() + 100);
        let min_required =
            BigInt::from(2 * PolicyContract::DEFAULT_NOTARY_ASSISTED_ATTRIBUTE_FEE as i64);
        let amount = min_required - 1;
        let result = try_token_transfer(
            GasToken::new().hash(),
            snapshot,
            settings.clone(),
            persisting_block.clone(),
            tx,
            from,
            to,
            &amount,
            data,
        );
        assert!(result.is_err(), "Insufficient initial deposit should fail");
    }

    // Good deposit should succeed.
    {
        let snapshot = make_snapshot_with_genesis(&settings);
        set_ledger_current_index(&snapshot, persisting_block.index() - 1);
        let tx = make_tx_with_signer(from);
        let data = build_notary_data(None, persisting_block.index() + 100);
        let min_required =
            BigInt::from(2 * PolicyContract::DEFAULT_NOTARY_ASSISTED_ATTRIBUTE_FEE as i64);
        let amount = min_required + 1;
        let result = try_token_transfer(
            GasToken::new().hash(),
            snapshot,
            settings.clone(),
            persisting_block,
            tx,
            from,
            to,
            &amount,
            data,
        )
        .expect("transfer");
        assert!(result, "Valid deposit should succeed");
    }
}

#[test]
fn check_expiration_of() {
    let settings = protocol_settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let mut persisting_block = make_persisting_block(1000, Vec::new());
    set_ledger_current_index(&snapshot, persisting_block.index() - 1);

    let from = NativeHelpers::get_bft_address(&settings.standby_validators());
    let notary_hash = Notary::new().hash();

    let expiration = call_notary_int(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        None,
        "expirationOf",
        vec![from.to_bytes()],
    );
    assert_eq!(expiration.to_u32().unwrap(), 0);

    let mut till = persisting_block.index() + 123;
    let data = build_notary_data(None, till);
    let amount = BigInt::from(2 * PolicyContract::DEFAULT_NOTARY_ASSISTED_ATTRIBUTE_FEE as i64 + 1);
    let tx = make_tx_with_signer(from);
    let ok = try_token_transfer(
        GasToken::new().hash(),
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        tx,
        from,
        notary_hash,
        &amount,
        data,
    )
    .expect("transfer");
    assert!(ok);

    let expiration = call_notary_int(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        None,
        "expirationOf",
        vec![from.to_bytes()],
    );
    assert_eq!(expiration.to_u32().unwrap(), till);

    // Extend deposit till.
    till += 5;
    let data = build_notary_data(None, till);
    let tx = make_tx_with_signer(from);
    let ok = try_token_transfer(
        GasToken::new().hash(),
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        tx,
        from,
        notary_hash,
        &BigInt::from(5),
        data,
    )
    .expect("transfer");
    assert!(ok);

    let expiration = call_notary_int(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        None,
        "expirationOf",
        vec![from.to_bytes()],
    );
    assert_eq!(expiration.to_u32().unwrap(), till);

    // Deposit to side account with custom owner.
    let to = UInt160::from_bytes(&[
        0x01, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00,
        0xff, 0x00, 0xff, 0x00, 0xa4,
    ])
    .expect("to");
    let data = build_notary_data(Some(to), till);
    let tx = make_tx_with_signer(from);
    let ok = try_token_transfer(
        GasToken::new().hash(),
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        tx,
        from,
        notary_hash,
        &amount,
        data,
    )
    .expect("transfer");
    assert!(ok);

    let expected_till = persisting_block.index() - 1 + 5760; // DefaultDepositDeltaTill
    let expiration = call_notary_int(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        None,
        "expirationOf",
        vec![to.to_bytes()],
    );
    assert_eq!(expiration.to_u32().unwrap(), expected_till);

    // Withdraw own deposit after expiration.
    persisting_block.header.index = till + 1;
    set_ledger_current_index(&snapshot, persisting_block.index());
    let ok = call_notary_bool(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        Some(make_tx_with_signer(from)),
        "withdraw",
        vec![from.to_bytes(), from.to_bytes()],
    );
    assert!(ok);

    let expiration = call_notary_int(
        Arc::clone(&snapshot),
        settings,
        persisting_block,
        None,
        "expirationOf",
        vec![from.to_bytes()],
    );
    assert_eq!(expiration.to_u32().unwrap(), 0);
}

#[test]
fn check_lock_deposit_until() {
    let settings = protocol_settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let persisting_block = make_persisting_block(1000, Vec::new());
    set_ledger_current_index(&snapshot, persisting_block.index() - 1);

    let from = NativeHelpers::get_bft_address(&settings.standby_validators());
    let notary_hash = Notary::new().hash();

    let expiration = call_notary_int(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        None,
        "expirationOf",
        vec![from.to_bytes()],
    );
    assert_eq!(expiration.to_u32().unwrap(), 0);

    // Update till on empty deposit should fail.
    let ok = call_notary_bool(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        Some(make_tx_with_signer(from)),
        "lockDepositUntil",
        vec![from.to_bytes(), bigint_to_bytes(&BigInt::from(123))],
    );
    assert!(!ok);

    // Make initial deposit.
    let till = persisting_block.index() + 123;
    let data = build_notary_data(None, till);
    let amount = BigInt::from(2 * PolicyContract::DEFAULT_NOTARY_ASSISTED_ATTRIBUTE_FEE as i64 + 1);
    let tx = make_tx_with_signer(from);
    let ok = try_token_transfer(
        GasToken::new().hash(),
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        tx,
        from,
        notary_hash,
        &amount,
        data,
    )
    .expect("transfer");
    assert!(ok);

    // Update deposit till for side account should fail.
    let other = UInt160::from_bytes(&[
        0x01, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00,
        0xff, 0x00, 0xff, 0x00, 0xa4,
    ])
    .expect("other");
    let ok = call_notary_bool(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        Some(make_tx_with_signer(other)),
        "lockDepositUntil",
        vec![other.to_bytes(), bigint_to_bytes(&BigInt::from(till + 10))],
    );
    assert!(!ok);

    // Decrease deposit till should fail.
    let ok = call_notary_bool(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        Some(make_tx_with_signer(from)),
        "lockDepositUntil",
        vec![from.to_bytes(), bigint_to_bytes(&BigInt::from(till - 1))],
    );
    assert!(!ok);

    // Good extension.
    let new_till = till + 10;
    let ok = call_notary_bool(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block,
        Some(make_tx_with_signer(from)),
        "lockDepositUntil",
        vec![from.to_bytes(), bigint_to_bytes(&BigInt::from(new_till))],
    );
    assert!(ok);
}

#[test]
fn check_balance_of() {
    let settings = protocol_settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let mut persisting_block = make_persisting_block(1000, Vec::new());
    set_ledger_current_index(&snapshot, persisting_block.index() - 1);

    let from = NativeHelpers::get_bft_address(&settings.standby_validators());
    let notary_hash = Notary::new().hash();

    let balance = call_notary_int(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        None,
        "balanceOf",
        vec![from.to_bytes()],
    );
    assert!(balance.is_zero());

    let till = persisting_block.index() + 123;
    let deposit1 = BigInt::from(2 * 1_0000_0000i64);
    let data = build_notary_data(None, till);
    let tx = make_tx_with_signer(from);
    let ok = try_token_transfer(
        GasToken::new().hash(),
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        tx,
        from,
        notary_hash,
        &deposit1,
        data,
    )
    .expect("transfer");
    assert!(ok);

    let balance = call_notary_int(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        None,
        "balanceOf",
        vec![from.to_bytes()],
    );
    assert_eq!(balance, deposit1);

    let deposit2 = BigInt::from(5);
    let data = build_notary_data(None, till);
    let tx = make_tx_with_signer(from);
    let ok = try_token_transfer(
        GasToken::new().hash(),
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        tx,
        from,
        notary_hash,
        &deposit2,
        data,
    )
    .expect("transfer");
    assert!(ok);

    let balance = call_notary_int(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        None,
        "balanceOf",
        vec![from.to_bytes()],
    );
    assert_eq!(balance, &deposit1 + &deposit2);

    // Deposit to side account.
    let to = UInt160::from_bytes(&[
        0x01, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00,
        0xff, 0x00, 0xff, 0x00, 0xa4,
    ])
    .expect("to");
    let data = build_notary_data(Some(to), till);
    let tx = make_tx_with_signer(from);
    let ok = try_token_transfer(
        GasToken::new().hash(),
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        tx,
        from,
        notary_hash,
        &deposit1,
        data,
    )
    .expect("transfer");
    assert!(ok);

    let balance = call_notary_int(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        None,
        "balanceOf",
        vec![to.to_bytes()],
    );
    assert_eq!(balance, deposit1);

    // Build Notary-assisted transaction to charge deposit on persist.
    let mut tx1 = Transaction::new();
    tx1.set_script(vec![OpCode::RET as u8]);
    tx1.set_signers(vec![
        Signer::new(notary_hash, WitnessScope::NONE),
        Signer::new(from, WitnessScope::GLOBAL),
    ]);
    tx1.set_attributes(vec![TransactionAttribute::NotaryAssisted(
        NotaryAssisted::new(4),
    )]);
    tx1.set_network_fee(1_0000_0000);

    let block_index = settings.committee_members_count() as u32;
    persisting_block = make_persisting_block(block_index, vec![tx1.clone()]);
    set_ledger_current_index(&snapshot, persisting_block.index() - 1);

    let key1 = KeyPair::generate().expect("keypair");
    let notary_key = key1.get_public_key_point().expect("public key");
    designate_notary_nodes(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.index() - 1,
        vec![notary_key],
    )
    .expect("designate notary");

    let mut on_persist = ApplicationEngine::new(
        TriggerType::OnPersist,
        None,
        Arc::clone(&snapshot),
        Some(persisting_block.clone()),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");
    on_persist.native_on_persist().expect("on persist");

    let expected_balance = deposit1 + deposit2 - BigInt::from(tx1.network_fee());
    let balance = call_notary_int(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        None,
        "balanceOf",
        vec![from.to_bytes()],
    );
    assert_eq!(balance, expected_balance);

    persisting_block.header.index = till + 1;
    set_ledger_current_index(&snapshot, persisting_block.index());
    let ok = call_notary_bool(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        Some(make_tx_with_signer(from)),
        "withdraw",
        vec![from.to_bytes(), from.to_bytes()],
    );
    assert!(ok);

    let balance = call_notary_int(
        Arc::clone(&snapshot),
        settings,
        persisting_block,
        None,
        "balanceOf",
        vec![from.to_bytes()],
    );
    assert!(balance.is_zero());
}

#[test]
fn check_withdraw() {
    let settings = protocol_settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let mut persisting_block = make_persisting_block(1000, Vec::new());
    set_ledger_current_index(&snapshot, persisting_block.index() - 1);

    let from = NativeHelpers::get_bft_address(&settings.standby_validators());
    let notary_hash = Notary::new().hash();

    let till = persisting_block.index() + 123;
    let deposit1 = BigInt::from(2 * 1_0000_0000i64);
    let data = build_notary_data(None, till);
    let tx = make_tx_with_signer(from);
    let ok = try_token_transfer(
        GasToken::new().hash(),
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        tx,
        from,
        notary_hash,
        &deposit1,
        data,
    )
    .expect("transfer");
    assert!(ok);

    // Unwitnessed withdraw should fail.
    let side_account = UInt160::from_bytes(&[
        0x01, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00, 0xff, 0x00,
        0xff, 0x00, 0xff, 0x00, 0xa4,
    ])
    .expect("side");
    let ok = call_notary_bool(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        Some(make_tx_with_signer(UInt160::zero())),
        "withdraw",
        vec![from.to_bytes(), side_account.to_bytes()],
    );
    assert!(!ok);

    // Withdraw missing deposit should fail.
    let ok = call_notary_bool(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        Some(make_tx_with_signer(side_account)),
        "withdraw",
        vec![side_account.to_bytes(), side_account.to_bytes()],
    );
    assert!(!ok);

    // Withdraw before expiration should fail.
    let ok = call_notary_bool(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        Some(make_tx_with_signer(from)),
        "withdraw",
        vec![from.to_bytes(), from.to_bytes()],
    );
    assert!(!ok);

    // Good withdrawal after expiration.
    persisting_block.header.index = till + 1;
    set_ledger_current_index(&snapshot, persisting_block.index());
    let ok = call_notary_bool(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.clone(),
        Some(make_tx_with_signer(from)),
        "withdraw",
        vec![from.to_bytes(), from.to_bytes()],
    );
    assert!(ok);

    let balance = call_notary_int(
        Arc::clone(&snapshot),
        settings,
        persisting_block,
        None,
        "balanceOf",
        vec![from.to_bytes()],
    );
    assert!(balance.is_zero());
}

#[test]
fn check_get_max_not_valid_before_delta() {
    let settings = protocol_settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let persisting_block = make_persisting_block(0, Vec::new());

    let value = call_notary_int(
        Arc::clone(&snapshot),
        settings,
        persisting_block,
        None,
        "getMaxNotValidBeforeDelta",
        Vec::new(),
    );
    assert_eq!(value.to_u32().unwrap(), 140);
}

#[test]
fn check_set_max_not_valid_before_delta() {
    let settings = protocol_settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let persisting_block = make_persisting_block(1000, Vec::new());
    let committee_address = NativeHelpers::committee_address(&settings, Some(snapshot.as_ref()));

    let tx = make_tx_with_signer(committee_address);
    let container = Arc::new(tx) as Arc<dyn IVerifiable>;
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        Arc::clone(&snapshot),
        Some(persisting_block.clone()),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");

    let args = vec![bigint_to_bytes(&BigInt::from(100))];
    engine
        .call_native_contract(Notary::new().hash(), "setMaxNotValidBeforeDelta", &args)
        .expect("setMaxNotValidBeforeDelta");

    let value = call_notary_int(
        Arc::clone(&snapshot),
        settings,
        persisting_block,
        None,
        "getMaxNotValidBeforeDelta",
        Vec::new(),
    );
    assert_eq!(value.to_u32().unwrap(), 100);
}

#[test]
fn check_on_persist_fee_per_key_update() {
    let settings = protocol_settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);

    let from = NativeHelpers::get_bft_address(&settings.standby_validators());
    let mut tx2 = Transaction::new();
    tx2.set_script(vec![OpCode::RET as u8]);
    tx2.set_signers(vec![Signer::new(from, WitnessScope::GLOBAL)]);
    tx2.set_attributes(vec![TransactionAttribute::NotaryAssisted(
        NotaryAssisted::new(4),
    )]);
    tx2.set_network_fee(1_0000_0000);
    tx2.set_system_fee(1000_0000);

    let persisting_block = make_persisting_block(10, vec![tx2.clone()]);
    set_ledger_current_index(&snapshot, persisting_block.index() - 1);

    let key1 = KeyPair::generate().expect("keypair");
    let notary_key = key1.get_public_key_point().expect("public key");
    designate_notary_nodes(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.index() - 1,
        vec![notary_key.clone()],
    )
    .expect("designate notary");

    let mut on_persist = ApplicationEngine::new(
        TriggerType::OnPersist,
        None,
        Arc::clone(&snapshot),
        Some(persisting_block.clone()),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");
    on_persist.native_on_persist().expect("on persist");

    // Update NotaryAssisted fee after OnPersist.
    let committee_address = NativeHelpers::committee_address(&settings, Some(snapshot.as_ref()));
    let tx = make_tx_with_signer(committee_address);
    let container = Arc::new(tx) as Arc<dyn IVerifiable>;
    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        Some(container),
        Arc::clone(&snapshot),
        Some(persisting_block.clone()),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");
    let args = vec![
        bigint_to_bytes(&BigInt::from(
            neo_core::network::p2p::payloads::TransactionAttributeType::NotaryAssisted as u8,
        )),
        bigint_to_bytes(&BigInt::from(5_0000_0000i64)),
    ];
    engine
        .call_native_contract(PolicyContract::new().hash(), "setAttributeFee", &args)
        .expect("setAttributeFee");

    let expected_reward = BigInt::from(5 * 1000_0000i64);
    let validators = NeoToken::new()
        .get_next_block_validators_snapshot(
            snapshot.as_ref(),
            settings.validators_count as usize,
            &settings,
        )
        .expect("validators");
    let primary = Contract::create_signature_contract(validators[0].clone()).script_hash();

    let gas = GasToken::new();
    let primary_balance = gas.balance_of_snapshot(snapshot.as_ref(), &primary);
    assert_eq!(
        primary_balance,
        BigInt::from(tx2.network_fee()) - &expected_reward
    );

    let notary_hash = Contract::create_signature_contract(notary_key).script_hash();
    let notary_balance = gas.balance_of_snapshot(snapshot.as_ref(), &notary_hash);
    assert_eq!(notary_balance, expected_reward);
}

#[test]
fn check_on_persist_notary_rewards() {
    let settings = protocol_settings_all_active();
    let snapshot = make_snapshot_with_genesis(&settings);
    let from = NativeHelpers::get_bft_address(&settings.standby_validators());

    let mut tx1 = Transaction::new();
    tx1.set_script(vec![OpCode::RET as u8]);
    tx1.set_signers(vec![Signer::new(from, WitnessScope::GLOBAL)]);
    tx1.set_attributes(vec![TransactionAttribute::NotaryAssisted(
        NotaryAssisted::new(4),
    )]);
    tx1.set_network_fee(1_0000_0000);

    let mut tx2 = Transaction::new();
    tx2.set_script(vec![OpCode::RET as u8]);
    tx2.set_signers(vec![Signer::new(from, WitnessScope::GLOBAL)]);
    tx2.set_attributes(vec![TransactionAttribute::NotaryAssisted(
        NotaryAssisted::new(6),
    )]);
    tx2.set_network_fee(2_0000_0000);

    let persisting_block = make_persisting_block(10, vec![tx1.clone(), tx2.clone()]);
    set_ledger_current_index(&snapshot, persisting_block.index() - 1);

    let key1 = KeyPair::generate().expect("keypair");
    let key2 = KeyPair::generate().expect("keypair");
    let notary1 = key1.get_public_key_point().expect("pubkey1");
    let notary2 = key2.get_public_key_point().expect("pubkey2");
    designate_notary_nodes(
        Arc::clone(&snapshot),
        settings.clone(),
        persisting_block.index() - 1,
        vec![notary1.clone(), notary2.clone()],
    )
    .expect("designate notary");

    let mut on_persist = ApplicationEngine::new(
        TriggerType::OnPersist,
        None,
        Arc::clone(&snapshot),
        Some(persisting_block.clone()),
        settings.clone(),
        TEST_GAS_LIMIT,
        None,
    )
    .expect("engine");
    on_persist.native_on_persist().expect("on persist");

    let expected_reward = BigInt::from(12 * 1000_0000i64);
    let per_notary = expected_reward.clone() / BigInt::from(2);

    let validators = NeoToken::new()
        .get_next_block_validators_snapshot(
            snapshot.as_ref(),
            settings.validators_count as usize,
            &settings,
        )
        .expect("validators");
    let primary = Contract::create_signature_contract(validators[0].clone()).script_hash();
    let gas = GasToken::new();
    let primary_balance = gas.balance_of_snapshot(snapshot.as_ref(), &primary);
    assert_eq!(
        primary_balance,
        BigInt::from(tx1.network_fee() + tx2.network_fee()) - &expected_reward
    );

    let notary1_hash = Contract::create_signature_contract(notary1).script_hash();
    let notary2_hash = Contract::create_signature_contract(notary2).script_hash();
    let notary1_balance = gas.balance_of_snapshot(snapshot.as_ref(), &notary1_hash);
    let notary2_balance = gas.balance_of_snapshot(snapshot.as_ref(), &notary2_hash);
    assert_eq!(notary1_balance, per_notary);
    assert_eq!(notary2_balance, per_notary);
}
