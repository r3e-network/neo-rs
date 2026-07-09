use super::*;
use neo_execution::native_contract_provider::NativeContractProvider;
use neo_native_contracts::{GasToken, LedgerContract, PolicyContract};
use neo_payloads::{Signer, Witness};
use neo_primitives::{UInt256, WitnessScope};
use neo_serialization::BinarySerializer;
use neo_storage::{StorageItem, StorageKey};
use neo_vm::StackItem;
use neo_vm_rs::{ExecutionEngineLimits, OpCode};
use std::sync::Arc;

fn standard_native_provider() -> Arc<dyn NativeContractProvider> {
    Arc::new(neo_native_contracts::StandardNativeProvider::new())
}

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

fn seed_notary_deposit(snapshot: &DataCache, account: &UInt160, amount: i64, till: u32) {
    let item = StackItem::from_struct(vec![
        StackItem::from_int(BigInt::from(amount)),
        StackItem::from_int(BigInt::from(till)),
    ]);
    let bytes = BinarySerializer::serialize(&item, &ExecutionEngineLimits::default()).unwrap();
    let mut key = vec![1u8];
    key.extend_from_slice(&account.to_bytes());
    snapshot.add(
        StorageKey::new(neo_native_contracts::Notary::ID, key),
        StorageItem::from_bytes(bytes),
    );
}

fn seed_policy_fee_settings(snapshot: &DataCache, exec_fee_factor: i64) {
    snapshot.add(
        StorageKey::new(PolicyContract::ID, vec![10]),
        StorageItem::from_bytes(BigInt::from(1_000).to_signed_bytes_le()),
    );
    snapshot.add(
        StorageKey::new(PolicyContract::ID, vec![18]),
        StorageItem::from_bytes(BigInt::from(exec_fee_factor).to_signed_bytes_le()),
    );
}

fn native_provider_impl() -> &'static str {
    let provider = include_str!("../../admission/native_provider.rs");
    let start = provider
        .find("impl<P> AdmissionNativeProvider for NativeAdmissionProvider<P>")
        .expect("provider impl exists");
    &provider[start..]
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

fn notary_sponsored_shape_transaction(payer: UInt160) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_nonce(2);
    tx.set_system_fee(100);
    tx.set_network_fee(3_000_000);
    tx.set_valid_until_block(1);
    tx.set_script(vec![OpCode::PUSH1.byte()]);
    tx.set_signers(vec![
        Signer::new(
            neo_native_contracts::Notary::script_hash(),
            WitnessScope::NONE,
        ),
        Signer::new(payer, WitnessScope::NONE),
    ]);
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
        verify_state_dependent_with_native_provider(
            &tx,
            &snapshot,
            &ProtocolSettings::default(),
            &BigInt::from(0),
            false,
            standard_native_provider(),
        ),
        VerifyResult::UnableToVerify,
        "C# Policy.GetFeePerByte/GetExecFeeFactor index initialized storage; missing keys must not fall back to defaults and admit a transaction"
    );
}

#[test]
fn admission_verifier_accepts_concrete_native_provider_arc() {
    let snapshot = DataCache::new(false);
    seed_current_ledger(&snapshot, 0);
    let public_key = [2u8; 33];
    let account = UInt160::from_script(
        &neo_vm::script_builder::redeem_script::RedeemScript::signature_redeem_script(&public_key),
    );
    mint_gas(&snapshot, &account, 100_000_000);
    let tx = standard_shape_transaction(account);

    assert_eq!(
        verify_state_dependent_with_native_provider(
            &tx,
            &snapshot,
            &ProtocolSettings::default(),
            &BigInt::from(0),
            false,
            Arc::new(neo_native_contracts::StandardNativeProvider::new()),
        ),
        VerifyResult::UnableToVerify,
        "generic admission verification must accept a concrete provider Arc without forcing dyn erasure at the call site"
    );
}

#[test]
fn notary_sponsored_fee_check_uses_payer_deposit_not_notary_gas() {
    let snapshot = DataCache::new(false);
    seed_current_ledger(&snapshot, 0);
    seed_policy_fee_settings(&snapshot, 30);
    let payer = UInt160::from([0x42; 20]);
    seed_notary_deposit(&snapshot, &payer, 4_000_000, 100);
    let tx = notary_sponsored_shape_transaction(payer);

    let result = verify_state_dependent_with_native_provider(
        &tx,
        &snapshot,
        &ProtocolSettings::default(),
        &BigInt::from(0),
        false,
        standard_native_provider(),
    );

    assert_ne!(
        result,
        VerifyResult::InsufficientFunds,
        "Neo v3.10.1 TransactionVerificationContext checks Notary-sponsored fees against Signers[1]'s deposit, not Notary's GAS balance"
    );
}

#[test]
fn policy_blocked_reader_uses_native_contract_projection() {
    let provider = native_provider_impl();
    let start = provider
        .find("fn policy_is_blocked")
        .expect("reader exists");
    let end = provider[start..]
        .find("fn max_valid_until_block_increment")
        .map(|offset| start + offset)
        .expect("next reader exists");
    let reader = &provider[start..end];

    assert!(reader.contains("PolicyContract::is_blocked_snapshot"));
    assert!(!reader.contains("POLICY_PREFIX_BLOCKED_ACCOUNT"));
    assert!(!reader.contains("StorageKey::new(POLICY_CONTRACT_ID"));
}

#[test]
fn max_valid_until_block_increment_uses_native_policy_reader() {
    let provider = native_provider_impl();
    let start = provider
        .find("fn max_valid_until_block_increment")
        .expect("reader exists");
    let end = provider[start..]
        .find("fn fee_per_byte")
        .map(|offset| start + offset)
        .expect("test module follows the helper");
    let reader = &provider[start..end];

    assert!(reader.contains("get_max_valid_until_block_increment_snapshot"));
    assert!(!reader.contains("StorageKey::new(POLICY_CONTRACT_ID"));
    assert!(!reader.contains("POLICY_PREFIX_MAX_VALID_UNTIL_BLOCK_INCREMENT"));
}

#[test]
fn attribute_network_fee_delegates_to_payload_attribute_formula() {
    let source = include_str!("../../admission/verification.rs");
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

#[test]
fn non_standard_witness_verification_uses_explicit_native_provider() {
    let source = include_str!("../../admission/verification.rs");
    let start = source
        .find("pub fn verify_state_dependent_with_native_provider")
        .expect("provider-aware state-dependent verifier exists");
    let end = source[start..]
        .find("/// C# `TransactionAttribute.CalculateNetworkFee` dispatch.")
        .map(|offset| start + offset)
        .expect("attribute helper follows verifier");
    let verifier = &source[start..end];

    assert!(verifier.contains("Helper::verify_witness_with_native_provider"));
    assert!(verifier.contains("let provider: Arc<dyn NativeContractProvider>"));
    assert!(!verifier.contains("Helper::verify_witness("));
}

#[test]
fn ledger_reads_use_admission_provider_boundary() {
    let verifier = include_str!("../../admission/verification.rs");
    let attributes = include_str!("../../verification/attributes.rs");
    let provider = include_str!("../../admission/ledger_provider.rs");

    assert!(verifier.contains("AdmissionLedgerProvider"));
    assert!(verifier.contains("NativeAdmissionLedgerProvider::new()"));
    assert!(attributes.contains("AdmissionLedgerProvider"));
    assert!(!verifier.contains("LedgerContract::new()"));
    assert!(!attributes.contains("LedgerContract::new()"));
    assert!(provider.contains("struct NativeAdmissionLedgerProvider"));
    assert!(provider.contains("ledger: LedgerContract"));
}

#[test]
fn native_reads_use_admission_provider_boundary() {
    let verifier = include_str!("../../admission/verification.rs");
    let attributes = include_str!("../../verification/attributes.rs");
    let provider = include_str!("../../admission/native_provider.rs");

    assert!(verifier.contains("AdmissionNativeProvider"));
    assert!(verifier.contains("NativeAdmissionProvider::new(native_contract_provider.clone())"));
    assert!(
        verifier.contains("pub fn verify_state_dependent_with_native_provider<P>"),
        "admission verification should stay generic over the composed native provider"
    );
    assert!(
        !verifier.contains("StandardNativeProvider"),
        "admission verification should require an explicit NativeContractProvider instead of constructing the default provider internally"
    );
    assert!(attributes.contains("AdmissionNativeProvider"));
    assert!(!verifier.contains("PolicyContract::new()"));
    assert!(!attributes.contains("NeoToken::new()"));
    assert!(!attributes.contains("OracleContract::new()"));
    assert!(!attributes.contains("RoleManagement::new()"));

    assert!(provider.contains("trait AdmissionNativeProvider"));
    assert!(!provider.contains("trait AdmissionNativeProviderFactory"));
    assert!(!provider.contains("struct NativeAdmissionProviderFactory"));
    assert!(provider.contains("struct NativeAdmissionProvider<P: ?Sized>"));
    assert!(provider.contains("native_contract_provider: Arc<P>"));
    assert!(!provider.contains("native_contract_provider: Arc<dyn NativeContractProvider>"));
    assert!(provider.contains("get_native_contract_by_name(name)"));
    assert!(provider.contains("with_contract::<GasToken"));
    assert!(provider.contains("with_contract::<NeoToken"));
    assert!(provider.contains("native_contract(\"Notary\")"));
    assert!(provider.contains("with_contract::<Notary"));
    assert!(provider.contains("with_contract::<OracleContract"));
    assert!(provider.contains("with_contract::<PolicyContract"));
    assert!(provider.contains("with_contract::<RoleManagement"));
    assert!(!provider.contains("neo: NeoToken"));
    assert!(!provider.contains("oracle: OracleContract"));
    assert!(!provider.contains("policy: PolicyContract"));
    assert!(!provider.contains("roles: RoleManagement"));
}
