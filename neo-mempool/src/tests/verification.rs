use super::*;
use neo_payloads::{Signer, Witness};
use neo_primitives::{UInt256, WitnessScope};
use neo_serialization::BinarySerializer;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use neo_vm_rs::{ExecutionEngineLimits, OpCode};

fn seed_current_ledger(snapshot: &DataCache, index: u32) {
    let hash = UInt256::from_bytes(&[0u8; 32]).expect("zero hash");
    let bytes = LedgerContract::new()
        .serialize_hash_index_state(&hash, index)
        .expect("hash index state");
    snapshot.add(
        StorageKey::new(LedgerContract::ID, vec![12]),
        StorageItem::from_bytes(bytes),
    );
}

fn mint_gas(snapshot: &DataCache, account: &UInt160, datoshi: i64) {
    let item = StackItem::from_struct(vec![StackItem::from_int(BigInt::from(datoshi))]);
    let bytes = BinarySerializer::serialize(&item, &ExecutionEngineLimits::default()).unwrap();
    let mut key = vec![20u8];
    key.extend_from_slice(&account.to_bytes());
    snapshot.add(
        StorageKey::new(GasToken::ID, key),
        StorageItem::from_bytes(bytes),
    );
}

fn standard_shape_transaction(account: UInt160) -> Transaction {
    let public_key = [2u8; 33];
    let verification =
        neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public_key);
    let mut invocation = vec![OpCode::PUSHDATA1.byte(), 64];
    invocation.extend_from_slice(&[0u8; 64]);

    let mut tx = Transaction::new();
    tx.set_nonce(1);
    tx.set_system_fee(100);
    tx.set_network_fee(3_000_000);
    tx.set_valid_until_block(1);
    tx.set_script(vec![OpCode::PUSH1.byte()]);
    tx.set_signers(vec![Signer::new(account, WitnessScope::NONE)]);
    tx.set_witnesses(vec![Witness::new_with_scripts(invocation, verification)]);
    tx
}

#[test]
fn oracle_response_fee_sum_uses_csharp_unchecked_long_arithmetic() {
    let mut tx = Transaction::new();
    tx.set_system_fee(i64::MAX);
    tx.set_network_fee(1);

    assert!(super::attributes::oracle_response_gas_matches(
        &tx,
        i64::MIN
    ));
    assert!(!super::attributes::oracle_response_gas_matches(
        &tx,
        i64::MAX
    ));
}

#[test]
fn missing_core_policy_fee_settings_fail_closed() {
    let snapshot = DataCache::new(false);
    seed_current_ledger(&snapshot, 0);
    let public_key = [2u8; 33];
    let account = UInt160::from_script(
        &neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public_key),
    );
    mint_gas(&snapshot, &account, 100_000_000);
    let tx = standard_shape_transaction(account);

    assert_eq!(
        verify_state_dependent(
            &tx,
            &snapshot,
            &ProtocolSettings::default(),
            &BigInt::from(0),
            false,
        ),
        VerifyResult::UnableToVerify,
        "C# Policy.GetFeePerByte/GetExecFeeFactor index initialized storage; missing keys must not fall back to defaults and admit a transaction"
    );
}

#[test]
fn policy_blocked_reader_uses_native_contract_projection() {
    let source = include_str!("../verification.rs");
    let start = source.find("fn policy_is_blocked").expect("reader exists");
    let end = source[start..]
        .find("fn max_valid_until_block_increment")
        .map(|offset| start + offset)
        .expect("next reader exists");
    let reader = &source[start..end];

    assert!(reader.contains("PolicyContract::is_blocked_snapshot"));
    assert!(!reader.contains("POLICY_PREFIX_BLOCKED_ACCOUNT"));
    assert!(!reader.contains("StorageKey::new(POLICY_CONTRACT_ID"));
}

#[test]
fn max_valid_until_block_increment_uses_native_policy_reader() {
    let source = include_str!("../verification.rs");
    let start = source
        .find("fn max_valid_until_block_increment")
        .expect("reader exists");
    let end = source[start..]
        .find("/// C# `NativeContract.GAS.BalanceOf")
        .map(|offset| start + offset)
        .expect("test module follows the helper");
    let reader = &source[start..end];

    assert!(reader.contains("get_max_valid_until_block_increment_snapshot"));
    assert!(!reader.contains("StorageKey::new(POLICY_CONTRACT_ID"));
    assert!(!reader.contains("POLICY_PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT"));
}

#[test]
fn attribute_network_fee_delegates_to_payload_attribute_formula() {
    let source = include_str!("../verification.rs");
    let start = source
        .find("fn attribute_network_fee")
        .expect("attribute fee helper exists");
    let end = source[start..]
        .find("#[cfg(test)]")
        .map(|offset| start + offset)
        .expect("test module follows the helper");
    let helper = &source[start..end];

    assert!(helper.contains("attribute.calculate_network_fee(snapshot, tx)"));
    assert!(!helper.contains("saturating_mul"));
    assert!(!helper.contains("TransactionAttribute::Conflicts"));
    assert!(!helper.contains("TransactionAttribute::NotaryAssisted"));
}
