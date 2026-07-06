use super::*;
use neo_config::ProtocolSettings;

/// ProtocolSettings with HF_Echidna scheduled from genesis — the Notary
/// contract is Echidna-activated (C# `Notary.Activations`), so e2e calls at
/// height 0 need it enabled (mirrors C# `TestProtocolSettings`).
fn echidna_settings() -> ProtocolSettings {
    let mut settings = ProtocolSettings::default();
    settings.hardforks.insert(Hardfork::HfEchidna, 0);
    settings
}
use crate::test_support::deploy_native;
use crate::{LedgerContract, Role, RoleManagement};
use neo_crypto::Secp256r1Crypto;
use neo_execution::native_contract::build_native_contract_state;
use neo_execution::{ApplicationEngine, Contract, NativeContract};
use neo_payloads::{
    Block, Header, NotaryAssisted, Signer, Transaction, TransactionAttribute, Witness,
    get_sign_data,
};
use neo_primitives::{
    CallFlags, TransactionAttributeType, TriggerType, UInt160, UInt256, Verifiable, WitnessScope,
};
use neo_serialization::BinarySerializer;
use neo_storage::StorageItem;
use neo_storage::persistence::DataCache;
use neo_vm::StackItem;
use neo_vm::script_builder::ScriptBuilder;
use neo_vm_rs::{ExecutionEngineLimits, OpCode, VmState};
use num_bigint::BigInt;
use std::sync::Arc;

/// Writes a P2PNotary designation effective from block `index`: the
/// RoleManagement record `(role_byte, index_be)` -> BinarySerializer array
/// of compressed EC-point byte strings.
fn seed_notary_designation(cache: &DataCache, index: u32, pubkeys: &[Vec<u8>]) {
    let list = StackItem::from_array(
        pubkeys
            .iter()
            .map(|p| StackItem::from_byte_string(p.clone()))
            .collect::<Vec<_>>(),
    );
    let value = BinarySerializer::serialize(&list, &ExecutionEngineLimits::default()).unwrap();
    cache.add(
        RoleManagement::designation_key(Role::P2PNotary.as_byte(), index),
        StorageItem::from_bytes(value),
    );
}

fn seed_current_block(cache: &DataCache, index: u32) {
    let value = LedgerContract::new()
        .serialize_hash_index_state(&UInt256::default(), index)
        .expect("current block pointer");
    cache.add(
        LedgerContract::current_block_storage_key(),
        StorageItem::from_bytes(value),
    );
}

/// A snapshot with the Notary native deployed and (optionally) the given
/// compressed public keys designated as P2PNotary nodes from genesis.
fn seeded_snapshot(notary_pubkeys: &[Vec<u8>]) -> Arc<DataCache> {
    crate::install();
    let cache = DataCache::new(false);
    seed_current_block(&cache, 0);
    deploy_native(
        &cache,
        &build_native_contract_state(&Notary, &echidna_settings(), 0),
    );
    if !notary_pubkeys.is_empty() {
        seed_notary_designation(&cache, 0, notary_pubkeys);
    }
    Arc::new(cache)
}

/// Calls `verify(signature)` on the Notary via System.Contract.Call with
/// `container` as the script container; `signature: None` pushes Null.
/// Returns the final VM state and the Boolean result.
fn call_verify(
    snapshot: Arc<DataCache>,
    container: Option<Arc<dyn Verifiable>>,
    signature: Option<&[u8]>,
) -> (VmState, bool) {
    let mut builder = ScriptBuilder::new();
    match signature {
        Some(bytes) => {
            builder.emit_push(bytes);
        }
        None => {
            builder.emit_opcode(OpCode::PUSHNULL);
        }
    }
    builder.emit_push_int(1);
    builder.emit_pack();
    builder.emit_push_int(i64::from(CallFlags::ALL.bits()));
    builder.emit_push("verify".as_bytes());
    builder.emit_push(&Notary::script_hash().to_array());
    builder.emit_syscall("System.Contract.Call").expect("call");

    let mut engine = ApplicationEngine::new(
        TriggerType::Application,
        container,
        snapshot,
        None,
        echidna_settings(),
        10_00000000,
        None,
    )
    .expect("engine builds");
    engine
        .load_script(builder.to_array(), CallFlags::ALL, None)
        .expect("script loads");
    let state = engine.execute_allow_fault();
    let result = engine
        .result_stack()
        .peek(0)
        .ok()
        .and_then(|item| item.as_bool().ok())
        .unwrap_or(false);
    (state, result)
}

/// A transaction carrying a NotaryAssisted attribute with the given signers.
fn notary_assisted_tx(signers: Vec<Signer>) -> Transaction {
    let mut tx = Transaction::new();
    tx.set_valid_until_block(100);
    tx.set_script(vec![0x40]); // RET
    tx.set_signers(signers);
    tx.set_attributes(vec![TransactionAttribute::NotaryAssisted(
        NotaryAssisted::new(1),
    )]);
    tx.set_witnesses(vec![Witness::empty()]);
    tx
}

/// Signs the C# `tx.GetSignData(network)` payload with the secp256r1 key.
fn sign_tx(tx: &Transaction, private_key: &[u8; 32]) -> Vec<u8> {
    let sign_data = get_sign_data(tx, echidna_settings().network).unwrap();
    Secp256r1Crypto::sign(&sign_data, private_key)
        .unwrap()
        .to_vec()
}

#[test]
fn verify_accepts_designated_notary_signature() {
    let private_key = Secp256r1Crypto::generate_private_key();
    let pubkey = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
    let snapshot = seeded_snapshot(&[pubkey]);

    let payer = UInt160::from_bytes(&[0x05; 20]).unwrap();
    let tx = notary_assisted_tx(vec![Signer::new(payer, WitnessScope::NONE)]);
    let signature = sign_tx(&tx, &private_key);
    let container: Arc<dyn Verifiable> = Arc::new(tx);

    // A designated notary's signature over the tx sign-data verifies.
    let (state, ok) = call_verify(
        Arc::clone(&snapshot),
        Some(Arc::clone(&container)),
        Some(&signature),
    );
    assert_eq!(state, VmState::HALT, "verify must HALT");
    assert!(ok, "designated notary signature must verify");

    // Tampering with one byte invalidates it (still a clean false).
    let mut tampered = signature.clone();
    tampered[10] ^= 0xFF;
    let (state2, ok2) = call_verify(snapshot, Some(container), Some(&tampered));
    assert_eq!(state2, VmState::HALT);
    assert!(!ok2, "tampered signature must not verify");
}

#[test]
fn verify_rejects_missing_container_attribute_or_malformed_signature() {
    let private_key = Secp256r1Crypto::generate_private_key();
    let pubkey = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
    let snapshot = seeded_snapshot(&[pubkey]);
    let payer = UInt160::from_bytes(&[0x05; 20]).unwrap();

    // No script container -> false.
    let (state, ok) = call_verify(Arc::clone(&snapshot), None, Some(&[0u8; 64]));
    assert_eq!(state, VmState::HALT);
    assert!(!ok, "verify without a transaction container must be false");

    // A transaction WITHOUT the NotaryAssisted attribute -> false even with
    // a valid notary signature over its sign-data.
    let mut plain = Transaction::new();
    plain.set_valid_until_block(100);
    plain.set_script(vec![0x40]);
    plain.set_signers(vec![Signer::new(payer, WitnessScope::NONE)]);
    plain.set_witnesses(vec![Witness::empty()]);
    let signature = sign_tx(&plain, &private_key);
    let container: Arc<dyn Verifiable> = Arc::new(plain);
    let (state2, ok2) = call_verify(Arc::clone(&snapshot), Some(container), Some(&signature));
    assert_eq!(state2, VmState::HALT);
    assert!(!ok2, "verify requires the NotaryAssisted attribute");

    // Wrong signature length and Null signature -> false.
    let tx = notary_assisted_tx(vec![Signer::new(payer, WitnessScope::NONE)]);
    let container2: Arc<dyn Verifiable> = Arc::new(tx);
    let (state3, ok3) = call_verify(
        Arc::clone(&snapshot),
        Some(Arc::clone(&container2)),
        Some(&[1u8; 10]),
    );
    assert_eq!(state3, VmState::HALT);
    assert!(!ok3, "a 10-byte signature must be rejected");
    let (state4, ok4) = call_verify(snapshot, Some(container2), None);
    assert_eq!(state4, VmState::HALT);
    assert!(!ok4, "a Null signature must be rejected");
}

#[test]
fn verify_rejects_when_no_notary_nodes_designated() {
    let private_key = Secp256r1Crypto::generate_private_key();
    let snapshot = seeded_snapshot(&[]); // no designation

    let payer = UInt160::from_bytes(&[0x05; 20]).unwrap();
    let tx = notary_assisted_tx(vec![Signer::new(payer, WitnessScope::NONE)]);
    let signature = sign_tx(&tx, &private_key);
    let container: Arc<dyn Verifiable> = Arc::new(tx);

    let (state, ok) = call_verify(snapshot, Some(container), Some(&signature));
    assert_eq!(state, VmState::HALT);
    assert!(!ok, "no designated notaries -> false");
}

#[test]
fn verify_requires_scope_none_on_the_notary_signer() {
    let private_key = Secp256r1Crypto::generate_private_key();
    let pubkey = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
    let snapshot = seeded_snapshot(&[pubkey]);

    // The Notary-account signer (second, so Sender stays the payer) carries
    // a non-None scope -> false despite the valid signature.
    let payer = UInt160::from_bytes(&[0x05; 20]).unwrap();
    let tx = notary_assisted_tx(vec![
        Signer::new(payer, WitnessScope::NONE),
        Signer::new(Notary::script_hash(), WitnessScope::GLOBAL),
    ]);
    let signature = sign_tx(&tx, &private_key);
    let container: Arc<dyn Verifiable> = Arc::new(tx);
    let (state, ok) = call_verify(Arc::clone(&snapshot), Some(container), Some(&signature));
    assert_eq!(state, VmState::HALT);
    assert!(!ok, "a scoped Notary signer must be rejected");

    // Scope None on the Notary signer passes the check.
    let tx2 = notary_assisted_tx(vec![
        Signer::new(payer, WitnessScope::NONE),
        Signer::new(Notary::script_hash(), WitnessScope::NONE),
    ]);
    let signature2 = sign_tx(&tx2, &private_key);
    let container2: Arc<dyn Verifiable> = Arc::new(tx2);
    let (state2, ok2) = call_verify(snapshot, Some(container2), Some(&signature2));
    assert_eq!(state2, VmState::HALT);
    assert!(ok2, "a scope-None Notary signer must pass");
}

#[test]
fn verify_notary_paid_transactions_require_a_funding_deposit() {
    let private_key = Secp256r1Crypto::generate_private_key();
    let pubkey = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
    let payer = UInt160::from_bytes(&[0x06; 20]).unwrap();

    // Sender == Notary (fees paid from the payer's deposit): SystemFee +
    // NetworkFee = 10 must be covered by the deposit.
    let mut tx = notary_assisted_tx(vec![
        Signer::new(Notary::script_hash(), WitnessScope::NONE),
        Signer::new(payer, WitnessScope::NONE),
    ]);
    tx.set_system_fee(6);
    tx.set_network_fee(4);
    let signature = sign_tx(&tx, &private_key);
    let container: Arc<dyn Verifiable> = Arc::new(tx);

    // No deposit -> false.
    let snapshot = seeded_snapshot(&[pubkey]);
    let (state, ok) = call_verify(
        Arc::clone(&snapshot),
        Some(Arc::clone(&container)),
        Some(&signature),
    );
    assert_eq!(state, VmState::HALT);
    assert!(
        !ok,
        "a Notary-paid tx without a payer deposit must be false"
    );

    // An underfunded deposit (9 < 10) -> false.
    Notary::new()
        .write_deposit(&snapshot, &payer, &BigInt::from(9), 1000)
        .unwrap();
    let (state2, ok2) = call_verify(
        Arc::clone(&snapshot),
        Some(Arc::clone(&container)),
        Some(&signature),
    );
    assert_eq!(state2, VmState::HALT);
    assert!(!ok2, "an underfunded deposit must be false");

    // A deposit covering the fees exactly -> true.
    Notary::new()
        .write_deposit(&snapshot, &payer, &BigInt::from(10), 1000)
        .unwrap();
    let (state3, ok3) = call_verify(Arc::clone(&snapshot), Some(container), Some(&signature));
    assert_eq!(state3, VmState::HALT);
    assert!(ok3, "a funded deposit must verify");

    // A single-signer Notary-paid tx (Signers.Length != 2) -> false.
    let mut single =
        notary_assisted_tx(vec![Signer::new(Notary::script_hash(), WitnessScope::NONE)]);
    single.set_system_fee(6);
    single.set_network_fee(4);
    let sig_single = sign_tx(&single, &private_key);
    let container_single: Arc<dyn Verifiable> = Arc::new(single);
    let (state4, ok4) = call_verify(snapshot, Some(container_single), Some(&sig_single));
    assert_eq!(state4, VmState::HALT);
    assert!(!ok4, "Sender == Notary requires exactly two signers");
}

/// C# `Notary.OnManifestCompose` (Notary.cs:92-102): NEP-27 alone until
/// HF_Faun is enabled at the height, then NEP-27 + NEP-30.
#[test]
fn manifest_standards_gain_nep30_at_faun() {
    let echidna_only = build_native_contract_state(&Notary, &echidna_settings(), 0);
    assert_eq!(echidna_only.manifest.supported_standards, ["NEP-27"]);

    let mut settings = echidna_settings();
    settings.hardforks.insert(Hardfork::HfFaun, 10);
    let before = build_native_contract_state(&Notary, &settings, 9);
    assert_eq!(before.manifest.supported_standards, ["NEP-27"]);
    let after = build_native_contract_state(&Notary, &settings, 10);
    assert_eq!(after.manifest.supported_standards, ["NEP-27", "NEP-30"]);
}

/// Reads the GAS balance of `account` out of the NEP-17 account record
/// (`Struct[Integer(balance), ...]`), returning 0 when absent.
fn gas_balance(snapshot: &DataCache, account: &UInt160) -> BigInt {
    crate::GasToken::balance_of(snapshot, account).expect("decode GAS account balance")
}

/// C# `Notary.OnPersistAsync` (Notary.cs:61-90): a NotaryAssisted
/// transaction paid by the Notary debits the payer's deposit by
/// `SystemFee + NetworkFee`, and the per-notary reward `(nKeys + 1) *
/// GetAttributeFeeV1(NotaryAssisted) / notaries.Length` is minted to each
/// designated P2PNotary node. This is the reminting counterpart of the
/// NotaryAssisted share `GasToken::on_persist` withholds from the primary
/// network-fee mint.
#[test]
fn on_persist_debits_payer_deposit_and_mints_notary_reward() {
    let private_key = Secp256r1Crypto::generate_private_key();
    let pubkey = Secp256r1Crypto::derive_public_key(&private_key).unwrap();
    // Deploys Notary + designates one P2PNotary node effective from 0.
    let snapshot = seeded_snapshot(&[pubkey]);

    // Seed the Policy NotaryAssisted attribute fee (HF_Echidna default,
    // 0.1 GAS), the value `GetAttributeFeeV1` reads.
    const FEE: i64 = 1000_0000;
    snapshot.add(
        crate::PolicyContract::attribute_fee_key(
            TransactionAttributeType::NotaryAssisted.to_byte(),
        ),
        StorageItem::from_bytes(crate::bigint_to_storage_bytes(&BigInt::from(FEE))),
    );

    // Seed the payer's deposit (amount D, till T).
    let payer = UInt160::from_bytes(&[0x07; 20]).unwrap();
    let deposit_amount = BigInt::from(5_0000_0000i64); // 5 GAS
    Notary::new()
        .write_deposit(&snapshot, &payer, &deposit_amount, 1000)
        .unwrap();

    // A NotaryAssisted tx (nKeys = 1) paid by the Notary on behalf of the
    // payer: Signers = [Notary, payer].
    let notary_hash = Notary::script_hash();
    let mut tx = notary_assisted_tx(vec![
        Signer::new(notary_hash, WitnessScope::NONE),
        Signer::new(payer, WitnessScope::NONE),
    ]);
    tx.set_system_fee(1_0000_0000); // 1 GAS
    tx.set_network_fee(5000_0000); // 0.5 GAS
    let fees = tx.system_fee().wrapping_add(tx.network_fee());

    let mut header = Header::new();
    header.set_index(1);
    let block = Block::from_parts(header, vec![tx]);

    let mut engine = ApplicationEngine::new(
        TriggerType::OnPersist,
        None,
        Arc::clone(&snapshot),
        Some(block),
        echidna_settings(),
        0,
        None,
    )
    .expect("engine builds");
    NativeContract::on_persist(&Notary, &mut engine).expect("notary on_persist");

    // Payer deposit debited by SystemFee + NetworkFee; Till unchanged.
    let (amount_after, till_after) = Notary::new()
        .read_deposit(&snapshot, &payer)
        .expect("deposit read")
        .expect("deposit present");
    assert_eq!(amount_after, &deposit_amount - BigInt::from(fees));
    assert_eq!(till_after, 1000);

    // Reward minted to the single designated notary: nFees = nKeys + 1 = 2,
    // singleReward = 2 * FEE / 1.
    let notaries = RoleManagement::new()
        .get_designated_by_role_at(&snapshot, Role::P2PNotary, 1)
        .unwrap();
    assert_eq!(notaries.len(), 1);
    let notary_addr = UInt160::from_script(&Contract::create_signature_redeem_script(
        notaries[0].clone(),
    ));
    assert_eq!(gas_balance(&snapshot, &notary_addr), BigInt::from(2 * FEE));
}
